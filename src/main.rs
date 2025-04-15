#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use anyhow::Context;
use indexer::{handle_messages, index_documents, search_term, Config, ErrorHandler};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::{fs, thread};

use clap::Parser;

use indexer::server::run_server;

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
        path: Option<PathBuf>,
        #[clap(short = 'o', long = "output", help = "Path to index files directory")]
        output_directory: Option<PathBuf>,
    },
    /// Query some search term using the index
    Search {
        #[arg(short = 'i', long = "index", help = "Path to index files directory")]
        index_directory: Option<PathBuf>,
        #[arg(short = 'q', long = "query", help = "Query to search")]
        query: String,
        #[arg(short = 'o', long = "output", help = "Write result to file")]
        output_file: Option<PathBuf>,
        #[arg(short = 'c', long = "count", help = "Number of results")]
        result_count: Option<usize>,
    },
    /// Serve the search engine via http
    Serve {
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_directory: Option<PathBuf>,
        #[arg(short = 'p', long = "port", help = "Port number")]
        port: Option<u16>,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut index_dir = home::home_dir().unwrap_or(Path::new(".").to_path_buf());
    index_dir.push(".indexer");
    let error_handler = match args.log_file {
        Some(file) => ErrorHandler::File(file),
        None => {
            let mut log_file = index_dir.clone();
            log_file.push("logs");
            ErrorHandler::File(log_file)
        }
    };

    let (sender, receiver) = mpsc::channel();
    let sender = Arc::new(Mutex::new(sender));

    match args.command {
        Commands::Index {
            path,
            output_directory,
        } => {
            let filepath = match path {
                Some(p) => p,
                None => {
                    let current_dir = std::env::current_dir().context("get current directory")?;
                    current_dir
                }
            };

            if output_directory.is_some() {
                index_dir = output_directory.unwrap();
            }

            let cfg = Config {
                filepath,
                index_path: index_dir,
                error_handler,
                sender,
            };

            let err_handler = cfg.error_handler.clone();
            thread::spawn(move || loop {
                let _ = handle_messages(&receiver, err_handler.clone());
            });

            index_documents(&cfg)?;
        }
        Commands::Search {
            index_directory,
            query,
            output_file,
            result_count,
        } => {
            let index_file = match index_directory {
                Some(p) => p,
                None => index_dir,
            };
            let mut result = search_term(&query, &index_file)?;

            // i'm not really sure what i should do if
            // I get zero matches
            if result.is_empty() {
                eprintln!("Zero Results");
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
                if let Some(count) = result_count {
                    if result.len() > count {
                        result.truncate(count);
                    }
                }
                result.iter().for_each(|r| println!("{:?}", r));
            }
        }
        Commands::Serve {
            index_directory,
            port,
        } => {
            let port = port.unwrap_or(8765);
            if index_directory.is_some() {
                index_dir = index_directory.unwrap();
            }

            run_server(&index_dir, port, sender)?;
        }
    }
    Ok(())
}
