#![feature(path_file_prefix)]
pub mod html;
pub mod lexer;
pub mod parsers;
pub mod server;
pub mod tree;

use anyhow::Context;
use parsers::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rustc_hash::FxHashMap;
use stop_words::LANGUAGE;
use tree::{DocumentStore, MainIndex};

use std::{
    fs,
    io::{stderr, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, mpsc, Arc, Mutex},
    time::SystemTime,
};

pub struct Config {
    pub filepath: PathBuf,
    pub index_path: PathBuf,
    pub dump_format: DumpFormat,
    pub error_handler: ErrorHandler,
    pub sender: Arc<Mutex<mpsc::Sender<String>>>,
}

pub enum DumpFormat {
    Json,
    Bytes,
}

#[derive(Clone)]
pub enum ErrorHandler {
    Stderr,
    File(PathBuf),
}

pub fn search_term(term: &str, index_file: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = lexer::Lexer::new(&text_chars);
    let stop_words = stop_words::get(LANGUAGE::English);
    let tokens = lex.get_tokens(&stop_words);
    let main_index = MainIndex::new(index_file, 100).context("new main index")?;
    let results = main_index.search(&tokens).context("query results")?;
    let results: Vec<PathBuf> = results.iter().map(|(path, _)| path.to_path_buf()).collect();
    Ok(results)
}

pub fn index_documents(cfg: &Config) -> anyhow::Result<()> {
    println!("Indexing documents...");
    let filepath = PathBuf::from(&cfg.filepath);
    let docs = if filepath.is_dir() {
        read_files_recursively(&filepath)?
    } else {
        Vec::from([filepath])
    };

    // let index_table = get_index_table(&cfg.index_path).unwrap_or_default();

    let mut extensions_map: FxHashMap<
        String,
        fn(&Path, Arc<Mutex<mpsc::Sender<String>>>, &[String]) -> anyhow::Result<Vec<String>>,
    > = FxHashMap::default();

    extensions_map.insert("csv".to_string(), parse_csv_document);
    extensions_map.insert("html".to_string(), parse_html_document);
    extensions_map.insert("pdf".to_string(), parse_pdf_document);
    extensions_map.insert("xml".to_string(), parse_xml_document);
    extensions_map.insert("xhtml".to_string(), parse_xml_document);
    extensions_map.insert("text".to_string(), parse_txt_document);
    extensions_map.insert("md".to_string(), parse_txt_document);

    // process the documents in parallel
    let model = Arc::new(Mutex::new(
        MainIndex::new(&cfg.index_path, 100).context("new main index")?,
    ));
    let skipped_files = AtomicU64::new(0);
    let indexed_files = AtomicU64::new(0);
    let stop_words = stop_words::get(LANGUAGE::English);
    let err_sender = Arc::clone(&cfg.sender);

    docs.par_iter().for_each(|doc| {
        // check if document index exists in the index_table;
        // if it exixts, check whether the file has been modified
        // since the last index
        // if yes then reindex the file
        // if no then skip the file
        let model = Arc::clone(&model);
        {
            let doc_id = model.lock().unwrap().doc_store.get_id(doc);
            if let Some(expired) = doc_index_is_expired(doc_id, &model.lock().unwrap().doc_store) {
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

    // if indexed_files.load(std::sync::atomic::Ordering::SeqCst) == 0 {
    //     return Ok(());
    // }
    // update the models idf
    // model.lock().unwrap().update_idf();

    // println!("Writing into {:?}...", cfg.index_path);
    // // write the documents index_table in the provided file path
    // let file = BufWriter::new(File::create(&cfg.index_path)?);
    //
    // match cfg.dump_format {
    //     DumpFormat::Json => serde_json::to_writer(file, &model.lock().unwrap().index_table)
    //         .context("serialise model into json")?,
    //     DumpFormat::Bytes => bincode2::serialize_into(file, &model.lock().unwrap().index_table)
    //         .context("serializing model into bytes")?,
    // };

    Ok(())
}

pub fn handle_messages(
    receiver: &mpsc::Receiver<String>,
    error_handler: ErrorHandler,
) -> anyhow::Result<()> {
    let mut messages = Vec::with_capacity(100);
    for _ in 0..100 {
        match receiver.recv() {
            Ok(message) => messages.push(message),
            Err(_) => break,
        }
    }

    if messages.is_empty() {
        return Ok(());
    }
    let mut messages = messages.join("\n");
    messages.push_str("\n");
    let messages = messages.as_bytes();

    match error_handler {
        ErrorHandler::Stderr => {
            let mut stderr = stderr().lock();
            stderr.write_all(&messages).context("write to stderr")?;
            stderr.flush().context("flush stderr")?;
        }
        ErrorHandler::File(f) => {
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&f)
                .context("opening log file")?;

            let mut writer = BufWriter::new(file);
            writer
                .write_all(&messages)
                .context("write buf to log file")?;

            writer.flush().context("flush log file buffer")?;
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
