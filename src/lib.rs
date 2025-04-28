#![feature(path_file_prefix)]

pub mod html;
pub mod lexer;
pub mod parsers;
pub mod server;
pub mod tree;

use anyhow::Context;
use indicatif::ProgressBar;
use parsers::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use stop_words::LANGUAGE;
use tree::{DocumentStore, MainIndex};

use std::{
    collections::HashMap,
    fs,
    io::{stderr, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, mpsc, Arc, Mutex},
    time::SystemTime,
};

pub struct Config {
    pub hidden: bool,                /* allow indexing hidden directories and files*/
    pub filepath: PathBuf,           /* filepath to perform indexing on*/
    pub index_path: PathBuf,         /* path to index directory*/
    pub error_handler: ErrorHandler, /* error output stream*/
    pub sender: Arc<Mutex<mpsc::Sender<String>>>, /*send errors*/
}

#[derive(Clone)]
pub enum ErrorHandler {
    Stderr,
    File(PathBuf),
}

pub fn search_term(term: &str, index_file: &Path) -> anyhow::Result<Vec<(PathBuf, f64)>> {
    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = lexer::Lexer::new(&text_chars);
    let stop_words = stop_words::get(LANGUAGE::English);
    let tokens = lex.get_tokens(&stop_words);
    let main_index = MainIndex::new(index_file).context("new main index")?;
    let results = main_index.search(&tokens).context("query results")?;
    Ok(results)
}

pub fn index_documents(cfg: &Config) -> anyhow::Result<()> {
    println!("Indexing documents...");
    let filepath = PathBuf::from(&cfg.filepath);
    let docs = if filepath.is_dir() {
        let basename = match filepath.file_name() {
            Some(v) => v.to_string_lossy().to_string(),
            None => "".to_string(),
        };
        if basename.starts_with(".") && !cfg.hidden {
            eprintln!("Provide the `hidden` flag to index hidden directories");
            return Ok(());
        }
        read_files_recursively(&filepath)?
    } else {
        Vec::from([filepath])
    };
    let bar = ProgressBar::new(docs.len() as u64);

    // let index_table = get_index_table(&cfg.index_path).unwrap_or_default();

    let mut extensions_map: HashMap<
        String,
        fn(&Path, Arc<Mutex<mpsc::Sender<String>>>, &[String]) -> anyhow::Result<Vec<String>>,
    > = HashMap::new();

    extensions_map.insert("csv".to_string(), parse_csv_document);
    extensions_map.insert("html".to_string(), parse_html_document);
    extensions_map.insert("pdf".to_string(), parse_pdf_document);
    extensions_map.insert("xml".to_string(), parse_xml_document);
    extensions_map.insert("xhtml".to_string(), parse_xml_document);
    extensions_map.insert("text".to_string(), parse_txt_document);
    extensions_map.insert("md".to_string(), parse_txt_document);
    extensions_map.shrink_to_fit();

    // process the documents in parallel
    let model = Arc::new(Mutex::new(
        MainIndex::new(&cfg.index_path).context("new main index")?,
    ));
    let skipped_files = AtomicU64::new(0);
    let indexed_files = AtomicU64::new(0);
    let stop_words = stop_words::get(LANGUAGE::English);
    let err_sender = Arc::clone(&cfg.sender);
    let total_size = AtomicU64::new(0);

    let chunk_size = 100;
    docs.chunks(chunk_size).for_each(|chunk| {
        chunk.par_iter().for_each(|doc| {
            // check if document index exists in the doc_store;
            // if it exixts, check whether the file has been modified
            // since the last time is was indexed
            // if yes then reindex the file
            // if no then skip the file
            let model = Arc::clone(&model);
            bar.inc(1);
            {
                match doc.extension() {
                    Some(v) => {
                        let v = v.to_string_lossy().to_string();
                        if extensions_map.get(&v).is_none() {
                            skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }
                    }
                    None => return,
                }
                let mut model = model.lock().unwrap();
                let doc_id = &model.doc_store.get_id(doc);
                if let Some(expired) = doc_index_is_expired(*doc_id, &model.doc_store) {
                    if !expired {
                        skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                }
            }
            if let Some(ext) = doc.extension() {
                let ext = ext.to_string_lossy().to_string();
                if let Some(parser) = extensions_map.get(&ext) {
                    match parser(doc, err_sender.clone(), &stop_words) {
                        Ok(tokens) => {
                            let mut model = model.lock().unwrap();
                            match model.add_document(doc, &tokens) {
                                Ok(_) => (),
                                Err(err) => eprintln!("ERROR: {}", err),
                            }
                            let file_size = doc.metadata().unwrap().len();
                            total_size.fetch_add(file_size, std::sync::atomic::Ordering::Relaxed);
                            indexed_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }
                        Err(err) => {
                            skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let _ = Arc::clone(&cfg.sender)
                                .lock()
                                .unwrap()
                                .send(format!("Skippped document: {:?}: {err}", doc));
                            return;
                        }
                    }
                }
            }

            skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = cfg
                .sender
                .lock()
                .unwrap()
                .send(format!("Failed to parse document: {:?}", doc));
        });
    });

    bar.finish();
    model.lock().unwrap().commit().context("commit model")?;
    println!("Completed Indexing documents...");
    println!(
        "Indexed {} files",
        indexed_files.load(std::sync::atomic::Ordering::SeqCst)
    );
    println!(
        "Skipped {} files",
        skipped_files.load(std::sync::atomic::Ordering::SeqCst)
    );

    println!(
        "Total files size: {} Mib",
        total_size.load(std::sync::atomic::Ordering::SeqCst) / 1024 / 1024
    );

    Ok(())
}

pub fn handle_messages(
    receiver: &mpsc::Receiver<String>,
    error_handler: ErrorHandler,
) -> anyhow::Result<()> {
    let message = match receiver.recv() {
        Ok(message) => message,
        Err(_) => return Ok(()),
    };

    match error_handler {
        ErrorHandler::Stderr => {
            let mut stderr = stderr().lock();
            let _ = stderr.write(message.as_bytes());
        }
        ErrorHandler::File(f) => {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&f)
                .context("opening log file")?;

            let _ = writeln!(file, "{}", message);
        }
    }
    Ok(())
}

fn read_files_recursively(files_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if files_dir.is_dir() {
        for entry in fs::read_dir(files_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let mut subdir_files = read_files_recursively(&path)?;
                files.append(&mut subdir_files);
            } else {
                files.push(path);
            }
        }
    } else {
        files.push(files_dir.to_path_buf());
    }

    Ok(files)
}

fn doc_index_is_expired(doc_id: u64, doc_store: &DocumentStore) -> Option<bool> {
    if let Some(doc_info) = doc_store.id_to_doc_info.get(&doc_id) {
        let now = SystemTime::now();
        let modified_at = Path::new(&doc_info.path)
            .metadata()
            .unwrap()
            .modified()
            .unwrap();
        let elapsed_since_modified = now.duration_since(modified_at).unwrap();
        let elapsed_since_indexed = now.duration_since(doc_info.indexed_at).unwrap();

        return Some(elapsed_since_indexed > elapsed_since_modified);
    };
    None
}
