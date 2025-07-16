use anyhow::{Context, anyhow};
use indexer::{Config, ErrorHandler, handle_messages, index_documents, search_term};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
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
        #[clap(
            short = 'z',
            long = "hidden",
            help = "Index hidden files and directories"
        )]
        hidden: bool,
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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut log_file = get_storage();
    log_file.push("logs");
    let error_handler = match args.log_file {
        Some(file) => ErrorHandler::File(file),
        None => ErrorHandler::File(log_file.clone()),
    };

    let (sender, receiver) = mpsc::channel();
    let sender = Arc::new(Mutex::new(sender));

    match args.command {
        Commands::Index {
            path,
            output_directory,
            hidden,
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
            };

            let err_handler = cfg.error_handler.clone();
            thread::spawn(move || {
                loop {
                    let _ = handle_messages(&receiver, err_handler.clone());
                }
            });

            index_documents(&cfg)?;
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
