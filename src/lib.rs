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
    io::{Write, stderr},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, atomic::AtomicU64, mpsc},
    time::{self, Duration, SystemTime},
};

/// Configuration for the indexing process.
pub struct Config {
    /// Allows indexing of hidden directories and files if `true`.
    pub hidden: bool,
    /// The handler for errors and informational messages.
    pub error_handler: ErrorHandler,
    /// The filepath or directory path to perform indexing on.
    pub filepath: PathBuf,
    /// The path to the directory where index files will be stored.
    pub index_path: PathBuf,
    /// A sender channel for sending messages (errors, info, debug).
    pub sender: Arc<RwLock<mpsc::Sender<Message>>>,
    /// A list of paths to skip during indexing.
    pub skip_paths: Vec<PathBuf>,
}

/// Defines where error and informational messages should be output.
#[derive(Clone)]
pub enum ErrorHandler {
    /// Messages are printed to `stderr`.
    Stderr,
    /// Messages are written to the specified file.
    File(PathBuf),
}

/// Represents different types of messages that can be sent through the message
/// channel.
pub enum Message {
    /// Signal to stop message handling.
    Break,
    /// An error message.
    Error(String),
    /// An informational message.
    Info(String),
    /// A debug message.
    Debug(String),
}

/// Type alias for a `HashMap` mapping file extensions (as `String`) to parser functions.
/// Each parser function takes a `Path`, an `Arc<RwLock<mpsc::Sender<Message>>>`,
/// and a slice of `String` (stop words), returning an `anyhow::Result<Vec<String>>`.
type ExtensionToParser = HashMap<
    String,
    fn(&Path, Arc<RwLock<mpsc::Sender<Message>>>, &[String]) -> anyhow::Result<Vec<String>>,
>;

fn get_extensions_map() -> ExtensionToParser {
    let mut extensions_map: ExtensionToParser = HashMap::new();

    extensions_map.insert("csv".to_string(), parse_csv_document);
    extensions_map.insert("html".to_string(), parse_html_document);
    extensions_map.insert("pdf".to_string(), parse_pdf_document);
    extensions_map.insert("xml".to_string(), parse_xml_document);
    extensions_map.insert("xhtml".to_string(), parse_xml_document);
    extensions_map.insert("txt".to_string(), parse_txt_document);
    extensions_map.insert("md".to_string(), parse_txt_document);
    extensions_map.shrink_to_fit();
    extensions_map
}

/// Searches the index for a given term. It tokenizes the term,
/// loads the main index, and performs the search.
///
/// # Arguments
/// * `term` - The search query string.
/// * `index_file` - The path to the directory containing the index files.
///
/// # Returns
/// A `Result` containing a `Vec` of tuples, where each tuple is a `PathBuf`
/// of a matching document and its TF-IDF score, or an `anyhow::Error` on failure.
pub fn search_term(term: &str, index_file: &Path) -> anyhow::Result<Vec<(PathBuf, f64)>> {
    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = lexer::Lexer::new(&text_chars);
    let stop_words = stop_words::get(LANGUAGE::English);
    let tokens = lex.get_tokens(&stop_words);
    let main_index = MainIndex::new(index_file).context("new main index")?;
    let results = main_index.search(&tokens).context("query results")?;
    Ok(results)
}

fn get_docs(
    filepath: PathBuf,
    handle_hidden: bool,
    skip_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    if filepath.is_dir() {
        let basename = match filepath.file_name() {
            Some(v) => v.to_string_lossy().to_string(),
            None => "".to_string(),
        };
        if basename.starts_with(".") && !handle_hidden {
            return Err("Provide the `hidden` flag to index hidden directories".to_string());
        }

        if skip_paths.contains(&filepath)
            || skip_paths.contains(&Path::new(&basename).to_path_buf())
        {
            return Err("Skipping and indexing the same path".to_string());
        }

        read_files_recursively(&filepath, handle_hidden, skip_paths)
    } else {
        Ok(Vec::from([filepath]))
    }
}

/// Recursively reads files from a directory, respecting hidden file settings
/// and skip paths.
///
/// # Arguments
/// * `files_dir` - The directory to read files from.
/// * `scan_hidden` - If `true`, hidden files and directories will be included.
/// * `skip_paths` - A slice of paths to explicitly skip.
///
/// # Returns
/// A `Result` containing a `Vec<PathBuf>` of discovered files, or an
/// `anyhow::Result` error.
fn read_files_recursively(
    files_dir: &Path,
    scan_hidden: bool,
    skip_paths: &[PathBuf],
) -> anyhow::Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    // Skip invalid filepaths
    // Skip hidden files if the scan_hidden flag is not set
    // Skip filepaths and basenames specified in the `skip_paths` list
    // Skip files whose executable bits have been set
    if !files_dir.exists() {
        return Ok(files);
    }

    let basename = files_dir
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();
    if (basename.starts_with(".") && !scan_hidden)
        || skip_paths.contains(&files_dir.to_path_buf())
        || skip_paths.contains(&Path::new(&basename).to_path_buf())
    {
        return Ok(files);
    }

    if files_dir.is_dir() {
        for entry in fs::read_dir(files_dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();

            let basename = path
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_default();
            if (basename.starts_with(".") && !scan_hidden)
                || skip_paths.contains(&path.to_path_buf())
                || skip_paths.contains(&Path::new(&basename).to_path_buf())
            {
                continue;
            }
            if path.is_dir() {
                let mut subdir_files = read_files_recursively(&path, scan_hidden, skip_paths)?;
                files.append(&mut subdir_files);
            } else {
                files.push(path);
            }
        }
    } else if let Ok(data) = fs::metadata(files_dir) {
        let mode = data.permissions().mode();
        // check execute bits set
        // (not set && push to files)
        if mode & 0o111 == 0 {
            files.push(files_dir.to_path_buf());
        }
    }

    Ok(files)
}

/// Checks if a document's index entry is expired, meaning the original file
/// has been modified more recently than it was indexed.
///
/// # Arguments
/// * `doc_id` - The ID of the document to check.
/// * `doc_store` - A reference to the `DocumentStore` containing document
///   metadata.
///
/// # Returns
/// `Some(true)` if the index is expired, `Some(false)` if not expired,
/// and `None` if the document ID is not found in the `doc_store`.
fn doc_index_is_expired(doc_id: u64, doc_store: &DocumentStore) -> bool {
    if let Some(doc_info) = doc_store.id_to_doc_info.get(&doc_id) {
        let now = SystemTime::now();
        let modified_at = Path::new(&doc_info.path)
            .metadata()
            .unwrap()
            .modified()
            .unwrap();
        let elapsed_since_modified = now.duration_since(modified_at).unwrap();
        let elapsed_since_indexed = now.duration_since(doc_info.indexed_at).unwrap();

        return elapsed_since_indexed > elapsed_since_modified;
    };
    true
}

fn process_doc(
    doc: &PathBuf,
    model: Arc<RwLock<MainIndex>>,
    err_sender: Arc<RwLock<mpsc::Sender<Message>>>,
    indexed_files: Arc<AtomicU64>,
    kilobytes: Arc<AtomicU64>,
    stop_words: &[String],
) {
    // check if document index exists in the doc_store;
    // if it exists, check whether the file has been modified
    // since the last time is was indexed
    // if yes then reindex the file
    // if no then skip the file
    let extensions_map = get_extensions_map();
    let ext = match doc.extension() {
        Some(v) => {
            let v = v.to_string_lossy().to_string();
            if !extensions_map.contains_key(&v) {
                return;
            }
            v
        }
        None => return,
    };

    {
        let doc_id = model.write().unwrap().doc_store.get_id(doc);
        if !doc_index_is_expired(doc_id, &model.read().unwrap().doc_store) {
            return;
        }
    }

    if let Some(parser) = extensions_map.get(&ext) {
        match parser(doc, Arc::clone(&err_sender), stop_words) {
            Ok(tokens) => {
                let file_size = doc.metadata().unwrap().len();
                // do the division here to prevent u64 overflow on large directories
                kilobytes.fetch_add(file_size / 1024, std::sync::atomic::Ordering::Relaxed);
                indexed_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                if let Err(err) = model.write().unwrap().add_document(doc, &tokens) {
                    let _ = err_sender.read().unwrap().send(Message::Error(format!(
                        "Error adding document to model: {err}"
                    )));
                }
                return;
            }
            Err(err) => {
                let _ = err_sender
                    .read()
                    .unwrap()
                    .send(Message::Info(format!("Skippped document: {doc:?}: {err}")));
                return;
            }
        }
    }

    let _ = err_sender
        .read()
        .unwrap()
        .send(Message::Error(format!("Failed to parse document: {doc:?}")));
}

/// Indexes documents located at `cfg.filepath`. It reads files recursively
/// (if it's a directory), parses them based on their extension, tokenizes the
/// content, and adds them to the index.
/// The process is parallelized for efficiency.
///
/// # Arguments
/// * `cfg` - A reference to the `Config` containing indexing parameters.
///
/// # Returns
/// `Ok(())` if indexing completes successfully, otherwise an `anyhow::Result` error.
pub fn index_documents(cfg: &Config) -> anyhow::Result<()> {
    println!("Indexing documents...");
    let filepath = PathBuf::from(&cfg.filepath);
    if !filepath.exists() {
        eprintln!("Provided an invalid filepath");
        return Ok(());
    }
    let docs =
        get_docs(filepath, cfg.hidden, &cfg.skip_paths).map_err(|err| anyhow::anyhow!(err))?;

    let bar = ProgressBar::new_spinner();
    bar.enable_steady_tick(Duration::from_millis(100));

    // process the documents in parallel
    let model = Arc::new(RwLock::new(
        MainIndex::new(&cfg.index_path).context("new main index")?,
    ));
    let indexed_files = Arc::new(AtomicU64::new(0));
    let stop_words = stop_words::get(LANGUAGE::English);
    let err_sender = Arc::clone(&cfg.sender);
    let kilobytes = Arc::new(AtomicU64::new(0));

    docs.par_iter().for_each(|doc| {
        process_doc(
            doc,
            Arc::clone(&model),
            Arc::clone(&err_sender),
            Arc::clone(&indexed_files),
            Arc::clone(&kilobytes),
            &stop_words,
        );
    });

    bar.finish();
    model.write().unwrap().commit().context("commit model")?;
    println!("Completed Indexing documents...");
    let indexed_files = indexed_files.load(std::sync::atomic::Ordering::SeqCst);
    println!(
        "Indexed {} file{}",
        indexed_files,
        if indexed_files == 1 { "" } else { "s" }
    );

    let kbs = kilobytes.load(std::sync::atomic::Ordering::SeqCst);
    let (mbs, kbs) = ((kbs / 1024), (kbs % 1024));
    println!("Total files size: {mbs} Mbs {kbs} Kbs");

    // Close the message handler
    let _ = Arc::clone(&cfg.sender).read().unwrap().send(Message::Break);
    Ok(())
}

/// Handles messages received from the indexing process, directing them to the
/// specified error handler.
/// Messages can be errors, informational, or debug messages.
///
/// # Arguments
/// * `receiver` - The `mpsc::Receiver` to receive messages from.
/// * `error_handler` - The `ErrorHandler` specifying where messages should be
///   output.
///
/// # Returns
/// `Ok(())` if message handling completes, or an `anyhow::Result` error if
/// writing to a file fails.
pub fn handle_messages(
    receiver: &mpsc::Receiver<Message>,
    error_handler: ErrorHandler,
) -> anyhow::Result<()> {
    while let Ok(message) = receiver.recv() {
        let now = SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let message = match message {
            Message::Break => return Ok(()),
            Message::Error(err) => format!("{now} INFO: {err}"),
            Message::Info(info) => format!("{now} INFO: {info}"),
            Message::Debug(deb) => format!("{now} INFO: {deb}"),
        };

        match error_handler {
            ErrorHandler::Stderr => {
                let mut stderr = stderr().lock();
                let _ = stderr.write_all(message.to_string().as_bytes());
            }
            ErrorHandler::File(ref f) => {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(f)
                    .context("opening log file")?;
                let _ = writeln!(file, "{message}");
            }
        }
    }
    Ok(())
}
