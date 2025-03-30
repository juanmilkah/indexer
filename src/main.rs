#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use indexer::{index_documents, search_term, DumpFormat, ErrorHandler, ErrorStream};
use std::fs;
use std::path::PathBuf;

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
