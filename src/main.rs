use std::char;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use clap::Parser;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use self::server::run_server;

mod html;
mod lexer;
mod models;
mod parsers;
mod server;

// use clap args parser instead
#[derive(Parser, Debug)]
#[command(
    name = "Indexer",
    about = "A minimalistic search engine",
    version = env!("CARGO_PKG_VERSION")
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Build an index for a directory
    Index {
        #[arg(
            short = 'd',
            long = "directory",
            help = "Directory to perfom action on"
        )]
        directory: String,
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_file: String,
    },
    /// Query some search term using the index
    Search {
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_file: String,
        #[arg(short = 'q', long = "query", help = "Query to search")]
        query: Option<String>,
    },
    /// Serve the search engine via http
    Serve {
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_file: String,
        #[arg(short = 'p', long = "port", help = "Port number")]
        port: Option<u32>,
    },
}

enum DocHandler {
    Indexed,
    Skipped,
}

fn search_term(term: &str, index_file: &str) -> io::Result<Vec<String>> {
    let file = BufReader::new(File::open(index_file)?);
    let index_table: models::IndexTable = serde_json::from_reader(file)?;

    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = lexer::Lexer::new(&text_chars);

    let mut tokens = Vec::new();

    while let Some(token) = lex.by_ref().next() {
        tokens.push(token);
    }

    let tokens = parsers::remove_stop_words(&tokens);
    let model = models::Model::new(index_table);
    let result = model.search_terms(&tokens);

    Ok(result)
}

fn read_files_recursively(files_dir: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();

    if files_dir.is_dir() {
        for entry in fs::read_dir(files_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let mut subdir_files = read_files_recursively(&path)?;
                files.append(&mut subdir_files);
            } else {
                files.push(path.to_string_lossy().to_string());
            }
        }
    }

    Ok(files)
}

fn get_index_table(filepath: &str) -> io::Result<models::IndexTable> {
    let index_file = File::open(filepath)?;
    let index_table: models::IndexTable = serde_json::from_reader(&index_file)?;
    Ok(index_table)
}

fn doc_index_is_expired(doc: &str, index_table: &models::IndexTable) -> Option<bool> {
    if let Some(doc_table) = index_table.tables.get(doc) {
        let now = SystemTime::now();
        let modified_at = Path::new(&doc).metadata().unwrap().modified().unwrap();
        let elapsed_since_modified = now.duration_since(modified_at).unwrap();
        let elapsed_since_indexed = now.duration_since(doc_table.indexed_at).unwrap();

        return Some(elapsed_since_indexed > elapsed_since_modified);
    };
    None
}

fn index_doc_by_extension(model: &mut models::Model, doc: &str) -> io::Result<DocHandler> {
    let doc_extension = Path::new(&doc).extension();
    match doc_extension {
        Some(ext) => match ext.to_str().unwrap() {
            "pdf" => match parsers::index_pdf_document(model, doc) {
                Ok(()) => Ok(DocHandler::Indexed),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    Err(err)
                }
            },
            "xml" | "xhtml" => match parsers::index_xml_document(model, doc) {
                Ok(()) => Ok(DocHandler::Indexed),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    Err(err)
                }
            },
            "html" => {
                let content = fs::read_to_string(doc)?;
                match parsers::index_html_document(model, &content, doc) {
                    Ok(()) => Ok(DocHandler::Indexed),
                    Err(err) => {
                        eprintln!("Failed to index {doc}: {err}");
                        Err(err)
                    }
                }
            }

            "txt" | "md" => match parsers::index_text_document(model, doc) {
                Ok(()) => Ok(DocHandler::Indexed),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    Err(err)
                }
            },
            _ => {
                eprintln!("Skipped {doc}");
                Ok(DocHandler::Skipped)
            }
        },
        None => Ok(DocHandler::Skipped),
    }
}

fn index_documents(files_dir: &str, index_path: &str) -> io::Result<()> {
    let files_dir = PathBuf::from(files_dir);
    let docs = read_files_recursively(&files_dir)?;
    let index_table = get_index_table(index_path).unwrap_or_else(|_| models::IndexTable::new());

    // process the documents in parallel
    let model = Arc::new(Mutex::new(models::Model::new(index_table)));
    let indexed_docs = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));

    docs.par_iter().for_each(|doc| {
        // check if document index exists in the index_table;
        // if it exixts, check whether the file has been modified
        // since the last index
        // if yes then reindex the file
        // if no then skip the file
        if let Some(is_expired) = doc_index_is_expired(doc, &model.lock().unwrap().index_table) {
            if !is_expired {
                println!("Skipped document: {doc}");
                skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        //match the document's file extension and index it accordingly
        match index_doc_by_extension(&mut model.lock().unwrap(), doc) {
            Ok(DocHandler::Skipped) => skipped.fetch_add(1, Ordering::Relaxed),
            Ok(DocHandler::Indexed) => indexed_docs.fetch_add(1, Ordering::Relaxed),
            Err(e) => {
                eprintln!("{e}");
                0
            }
        };
    });
    //
    // update the models idf
    model.lock().unwrap().update_idf();

    // write the documents index_table in the provided file path

    println!(
        "Indexed {indexed_docs} documents!",
        indexed_docs = indexed_docs.load(Ordering::Relaxed)
    );
    println!(
        "Skipped {skipped} documents!",
        skipped = skipped.load(Ordering::Relaxed)
    );
    println!("Writing into {index_path}...");
    let file = BufWriter::new(File::create(index_path)?);
    serde_json::to_writer(file, &model.lock().unwrap().index_table)?;

    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Index {
            directory,
            index_file,
        } => {
            index_documents(&directory, &index_file)?;
        }
        Commands::Search { index_file, query } => {
            if query.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Some fields missing for query",
                ));
            }
            search_term(&query.unwrap(), &index_file)?;
        }
        Commands::Serve { index_file, port } => {
            let port = port.unwrap_or(8765);

            run_server(&index_file, port)?;
        }
    }
    Ok(())
}
