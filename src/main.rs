#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::char;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::Context;
use clap::Parser;
use parsers::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rayon::slice::ParallelSlice;
use rustc_hash::FxHashMap;

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
    /// The key functionality commands
    #[command(subcommand)]
    command: Commands,

    /// Redirect Stderr and Stdout to a file descriptor
    #[arg(
        short = 'l',
        long = "log",
        help = "Redirect Stderr and Stdout to a file"
    )]
    log_file: Option<PathBuf>,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Build an index for a directory
    Index {
        #[clap(short = 'p', long = "path", help = "Path to perfom action on")]
        path: PathBuf,
        #[clap(short = 'o', long = "output", help = "Path to index file")]
        output_file: PathBuf,
        #[clap(
            short = 'j',
            long = "json",
            help = "Dump the file index in json format"
        )]
        json: bool,
    },
    /// Query some search term using the index
    Search {
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_file: PathBuf,
        #[arg(short = 'q', long = "query", help = "Query to search")]
        query: String,
        #[arg(short = 'o', long = "output", help = "Write result to file")]
        output_file: Option<PathBuf>,
    },
    /// Serve the search engine via http
    Serve {
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_file: PathBuf,
        #[arg(short = 'p', long = "port", help = "Port number")]
        port: Option<u16>,
    },
}

enum DumpFormat {
    Json,
    Bytes,
}

pub struct ErrorHandler {
    stream: ErrorStream,
}

enum ErrorStream {
    File(PathBuf),
    Stderr,
}

impl ErrorHandler {
    fn new(error_stream: ErrorStream) -> Self {
        Self {
            stream: error_stream,
        }
    }

    fn print(&mut self, err: &str) {
        match &self.stream {
            ErrorStream::Stderr => {
                eprintln!("{:?}", err);
            }
            ErrorStream::File(f) => {
                let mut file = match fs::OpenOptions::new().create(true).append(true).open(f) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                };

                match writeln!(&mut file, "{}", err) {
                    Ok(()) => (),
                    Err(e) => {
                        eprintln!("ERROR WRITING TO LOG FILE: {}", e);
                        eprintln!("{}", err);
                    }
                }
            }
        }
    }
}

fn search_term(term: &str, index_file: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = lexer::Lexer::new(&text_chars);

    let mut tokens = Vec::new();

    while let Some(token) = lex.by_ref().next() {
        tokens.push(token);
    }

    let tokens = parsers::remove_stop_words(&tokens);
    let index_table = get_index_table(index_file).context("get index table")?;
    let model = models::Model::new(index_table);
    Ok(model.search_terms(&tokens))
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

fn get_index_table(filepath: &Path) -> anyhow::Result<models::IndexTable> {
    let mut index_file = BufReader::new(File::open(filepath).context("open index file")?);
    let mut buf = Vec::new();
    index_file
        .read_to_end(&mut buf)
        .context("read index file")?;

    let dump_format = match buf[0] {
        b'{' => DumpFormat::Json,
        _ => DumpFormat::Bytes,
    };

    let index_table: models::IndexTable = match dump_format {
        DumpFormat::Bytes => {
            bincode2::deserialize(&buf).context("deserializing model from bytes")?
        }
        DumpFormat::Json => serde_json::from_slice(&buf).context("deserialise model from json")?,
    };

    Ok(index_table)
}

fn doc_index_is_expired(doc: &PathBuf, index_table: &models::IndexTable) -> Option<bool> {
    if let Some(doc_table) = index_table.tables.get(doc) {
        let now = SystemTime::now();
        let modified_at = Path::new(&doc).metadata().unwrap().modified().unwrap();
        let elapsed_since_modified = now.duration_since(modified_at).unwrap();
        let elapsed_since_indexed = now.duration_since(doc_table.indexed_at).unwrap();

        return Some(elapsed_since_indexed > elapsed_since_modified);
    };
    None
}

fn index_documents(
    filepath: &Path,
    index_path: &Path,
    dump_format: DumpFormat,
    error_handler: &mut ErrorHandler,
) -> anyhow::Result<()> {
    println!("Indexing documents...");
    let filepath = PathBuf::from(filepath);
    let docs = if filepath.is_dir() {
        read_files_recursively(&filepath)?
    } else {
        Vec::from([filepath])
    };

    let index_table = get_index_table(index_path).unwrap_or_default();

    let mut extensions_map: FxHashMap<
        String,
        fn(&Path, Arc<Mutex<&mut ErrorHandler>>) -> anyhow::Result<Vec<String>>,
    > = FxHashMap::default();

    extensions_map.insert("csv".to_string(), parse_csv_document);
    extensions_map.insert("html".to_string(), parse_html_document);
    extensions_map.insert("pdf".to_string(), parse_pdf_document);
    extensions_map.insert("xml".to_string(), parse_xml_document);
    extensions_map.insert("xhtml".to_string(), parse_xml_document);
    extensions_map.insert("text".to_string(), parse_txt_document);
    extensions_map.insert("md".to_string(), parse_txt_document);

    // process the documents in parallel
    let model = Arc::new(Mutex::new(models::Model::new(index_table)));
    let skipped_files = AtomicU64::new(0);
    let indexed_files = AtomicU64::new(0);
    let err_handler = Arc::new(Mutex::new(error_handler));

    let chunk_size = 100;
    docs.par_chunks(chunk_size).for_each(|chunk| {
        chunk.par_iter().for_each(|doc| {
            // check if document index exists in the index_table;
            // if it exixts, check whether the file has been modified
            // since the last index
            // if yes then reindex the file
            // if no then skip the file
            if let Some(is_expired) = doc_index_is_expired(doc, &model.lock().unwrap().index_table)
            {
                if !is_expired {
                    skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let mut err_handler = err_handler.lock().unwrap();
                    err_handler.print(&format!("Skipped document: {:?}", doc));
                    return;
                }
            }

            if let Some(ext) = doc.extension() {
                let ext = ext.to_string_lossy().to_string();
                if let Some(parser) = extensions_map.get(&ext) {
                    match parser(doc, Arc::clone(&err_handler)) {
                        Ok(tokens) => {
                            let mut model = model.lock().unwrap();
                            model.add_document(doc, &tokens);
                            indexed_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }
                        Err(err) => {
                            skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let mut err_handler = err_handler.lock().unwrap();
                            err_handler
                                .print(&format!("Failed to parse document: {:?}: {err}", doc));
                            return;
                        }
                    }
                }
            }

            skipped_files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut err_handler = err_handler.lock().unwrap();
            err_handler.print(&format!("Failed to parse document: {:?}", doc));
        });
    });

    // update the models idf
    model.lock().unwrap().update_idf();

    {
        println!("Completed Indexing!");
        println!("Writing into {:?}...", index_path);
    }
    // write the documents index_table in the provided file path
    let file = BufWriter::new(File::create(index_path)?);

    match dump_format {
        DumpFormat::Json => serde_json::to_writer(file, &model.lock().unwrap().index_table)
            .context("serialise model into json")?,
        DumpFormat::Bytes => bincode2::serialize_into(file, &model.lock().unwrap().index_table)
            .context("serializing model into bytes")?,
    };

    println!(
        "Indexed {} files",
        indexed_files.load(std::sync::atomic::Ordering::SeqCst)
    );
    println!(
        "Skipped {} files",
        skipped_files.load(std::sync::atomic::Ordering::SeqCst)
    );

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut error_handler = match args.log_file {
        Some(file) => ErrorHandler::new(ErrorStream::File(file)),
        None => ErrorHandler::new(ErrorStream::Stderr),
    };

    match args.command {
        Commands::Index {
            path,
            output_file,
            json,
        } => {
            let dump_format = if json {
                DumpFormat::Json
            } else {
                DumpFormat::Bytes
            };

            index_documents(&path, &output_file, dump_format, &mut error_handler)?;
        }
        Commands::Search {
            index_file,
            query,
            output_file,
        } => {
            let result = search_term(&query, &index_file)?;

            // i'm not really sure what i should do if
            // I get zero matches
            if result.is_empty() {
                error_handler.print(&format!("No Zero Matches!"));
                return Ok(());
            }

            if let Some(ref f) = output_file {
                let result = result
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect::<Vec<String>>()
                    .join("\n");
                fs::write(f, result)?;
            } else {
                result.iter().for_each(|r| println!("{:?}", r));
            }
        }
        Commands::Serve { index_file, port } => {
            let port = port.unwrap_or(8765);

            run_server(&index_file, port, &mut error_handler)?;
        }
    }
    Ok(())
}
