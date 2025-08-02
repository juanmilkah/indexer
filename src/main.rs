use anyhow::{Context, anyhow};
use indexer::{Config, ErrorHandler, handle_messages, index_documents, search_term};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, mpsc};
use std::{fs, thread};

use clap::Parser;

use indexer::server::run_server;

/// Represents the command-line arguments for the Indexer application.
#[derive(Parser, Debug)]
#[command(
    name = "Indexer",
    about = "A minimalistic search engine",
    version = env!("CARGO_PKG_VERSION")
)]
struct Args {
    /// The key functionality commands.
    #[command(subcommand)]
    command: Commands,

    /// Redirect Stderr and Stdout to a file descriptor.
    #[arg(
        short = 'l',
        long = "log",
        help = "Redirect Stderr and Stdout to a file"
    )]
    log_file: Option<PathBuf>,
}

/// Defines the available subcommands for the Indexer application.
#[derive(Parser, Debug)]
enum Commands {
    /// Build an index for a directory.
    Index {
        /// Path to perform action on.
        #[clap(short = 'p', long = "path", help = "Path to perfom action on")]
        path: Option<PathBuf>,
        /// Path to index files directory.
        #[clap(short = 'o', long = "output", help = "Path to index files directory")]
        output_directory: Option<PathBuf>,
        /// Index hidden files and directories.
        #[clap(
            short = 'z',
            long = "hidden",
            help = "Index hidden files and directories"
        )]
        hidden: bool,
        /// Skip paths with specified basename.
        /// To skip `target` directories:
        /// `indexer index --path . --skip-paths target`
        #[clap(
            short = 's',
            long = "skip-paths",
            help = "Skip specific entries: directories and files"
        )]
        skip_paths: Option<Vec<PathBuf>>,
    },
    /// Query some search term using the index.
    Search {
        /// Path to index files directory.
        #[arg(short = 'i', long = "index", help = "Path to index files directory")]
        index_directory: Option<PathBuf>,
        /// Query to search.
        #[arg(short = 'q', long = "query", help = "Query to search")]
        query: String,
        /// Write result to file.
        #[arg(short = 'o', long = "output", help = "Write result to file")]
        output_file: Option<PathBuf>,
        /// Number of results to return.
        #[arg(short = 'c', long = "count", help = "Number of results")]
        result_count: Option<usize>,
    },
    /// Serve the search engine via HTTP.
    Serve {
        /// Path to index file.
        #[arg(short = 'i', long = "index", help = "Path to index file")]
        index_directory: Option<PathBuf>,
        /// Port number to listen on.
        #[arg(short = 'p', long = "port", help = "Port number")]
        port: Option<u16>,
    },
}

/// Determines and returns the default storage directory for the indexer.
/// This will typically be `~/.indexer`. If the directory does not exist, it
/// attempts to create it.
///
/// # Returns
/// A `PathBuf` representing the storage directory.
fn get_storage() -> PathBuf {
    let mut index_dir = home::home_dir().unwrap_or(Path::new(".").to_path_buf());
    index_dir.push(".indexer");
    if !index_dir.exists() {
        fs::create_dir(&index_dir)
            .map_err(|err| eprintln!("Create .indexer dir: {err}"))
            .unwrap();
    }
    index_dir
}

/// The main entry point of the Indexer application.
/// It parses command-line arguments and dispatches to the appropriate
/// subcommand logic.
///
/// # Returns
/// `Ok(())` if the operation was successful, otherwise an `anyhow::Result` error.
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut log_file = get_storage();
    log_file.push("logs");
    let error_handler = match args.log_file {
        Some(file) => ErrorHandler::File(file),
        None => ErrorHandler::File(log_file.clone()),
    };

    let (sender, receiver) = mpsc::channel();
    let sender = Arc::new(RwLock::new(sender));

    match args.command {
        Commands::Index {
            path,
            output_directory,
            hidden,
            skip_paths,
        } => {
            let filepath = match path {
                Some(p) => p,
                None => std::env::current_dir().context("get current directory")?,
            };

            let index_path = {
                if let Some(path) = output_directory {
                    match fs::create_dir_all(&path) {
                        Ok(_) => (),
                        Err(err) => return Err(anyhow!(format!("ERROR: create ouput dir: {err}"))),
                    }
                    path
                } else {
                    get_storage()
                }
            };

            let cfg = Config {
                filepath,
                index_path,
                error_handler,
                sender,
                hidden,
                skip_paths: skip_paths.unwrap_or_default(),
            };

            let err_handler = cfg.error_handler.clone();
            // Spawns a new thread to handle messages (errors/info) from the
            // indexing process.
            let logs_handler = thread::spawn(move || {
                let _ = handle_messages(&receiver, err_handler.clone());
            });

            index_documents(&cfg)?;
            logs_handler.join().unwrap();
            println!("Logs saved to: {log_file:?}");
        }
        Commands::Search {
            index_directory,
            query,
            output_file,
            result_count,
        } => {
            let index_files = match index_directory {
                Some(p) => p,
                None => get_storage(),
            };
            let mut result = search_term(&query, &index_files)?;

            // i'm not really sure what i should do if
            // I get zero matches
            if result.is_empty() {
                eprintln!("Zero Results");
                return Ok(());
            }

            if let Some(ref f) = output_file {
                let result = result
                    .iter()
                    .map(|(path, score)| {
                        let path = path.to_string_lossy().to_string();
                        format!("{score}: {path}")
                    })
                    .collect::<Vec<String>>();
                let result = result.join("");
                fs::write(f, result)?;
            } else {
                if let Some(count) = result_count
                    && result.len() > count
                {
                    result.truncate(count);
                }
                result.iter().for_each(|(p, c)| println!("{c}: {p:?}"));
            }
        }
        Commands::Serve {
            index_directory,
            port,
        } => {
            let port = port.unwrap_or(8765);
            let index_files = match index_directory {
                Some(p) => p,
                None => get_storage(),
            };

            run_server(&index_files, port, sender)?;
        }
    }
    Ok(())
}
