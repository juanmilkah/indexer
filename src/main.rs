use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{char, env};

mod lexer;
mod models;
mod parsers;
mod server;

enum Commands {
    Query {
        index_file: String,
        term: String,
    },
    Index {
        files_dir: String,
        index_path: String,
    },
    Serve {
        index_file: String,
        port: u32,
    },
    Help,
    Version,
}

enum DocHandler {
    Indexed,
    Skipped,
}

fn entry() -> Result<Option<Commands>, ()> {
    // Match the commadline arguments
    //
    // Indexing files in a directory
    // indexer index ~/docs main_index.json
    //
    // Querying for a term from a directory's index
    // indexer query main_index.json "foo bar baz"
    //
    // Serving the results via http
    // indexer serve main_index.json 8989
    // curl http://localhost:8989/
    let mut args = env::args().skip(1).peekable();

    if let Some(subcommand) = args.next() {
        match subcommand.as_str() {
            "query" | "-q" => {
                if let Some(index_file) = args.next() {
                    if let Some(term) = args.next() {
                        Ok(Some(Commands::Query { term, index_file }))
                    } else {
                        usage();
                        Ok(None)
                    }
                } else {
                    usage();
                    Ok(None)
                }
            }
            "index" | "-i" => {
                // index the provided directory and write the documents index table
                // in the provided index file
                // otherwise fall back to the current directory and index.json respectively
                if let Some(dir) = args.next() {
                    if let Some(index) = args.next() {
                        Ok(Some(Commands::Index {
                            files_dir: dir,
                            index_path: index,
                        }))
                    } else {
                        Ok(Some(Commands::Index {
                            files_dir: dir,
                            index_path: "index.json".to_string(),
                        }))
                    }
                } else {
                    Ok(Some(Commands::Index {
                        files_dir: ".".to_string(),
                        index_path: "index.json".to_string(),
                    }))
                }
            }
            "serve" | "-s" => {
                if let Some(index_file) = args.next() {
                    if let Some(port) = args.next() {
                        let port = port.parse().unwrap_or(8080);
                        Ok(Some(Commands::Serve { index_file, port }))
                    } else {
                        Ok(Some(Commands::Serve {
                            index_file,
                            port: 8080,
                        }))
                    }
                } else {
                    eprintln!("Missing index file path");
                    Ok(None)
                }
            }

            "help" | "--help" | "-h" => Ok(Some(Commands::Help)),
            "version" | "--version" | "-v" => Ok(Some(Commands::Version)),
            _ => Ok(None),
        }
    } else {
        usage();
        Ok(None)
    }
}

fn usage() {
    println!("USAGE: [COMMANDS] [OPTIONS]");
    println!("Commands:");
    println!();
    println!("\t<query | -q> <index_path> <term>          Query for a term in documents");
    println!("\t<index | -i> <directory> [index_path]     Create an index from a directory");
    println!("\t<serve | -s> <index_path> [port]          Serve the responses to an http server");
    println!();
    println!("\t<help | -h | --help>                      Show program Usage");
    println!("\t<version | -v | --version>                Show the program version");
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

fn version_info() {
    println!("{version}", version = env!("CARGO_PKG_VERSION"));
}

fn get_index_table(filepath: &str) -> io::Result<models::IndexTable> {
    let index_file = File::open(filepath)?;
    let index_table: models::IndexTable = serde_json::from_reader(&index_file)?;
    Ok(index_table)
}

fn doc_index_is_expired(doc: &str, index_table: &models::IndexTable) -> Option<bool> {
    match index_table.tables.get(doc) {
        Some(doc_table) => {
            let now = SystemTime::now();
            let modified_at = Path::new(&doc).metadata().unwrap().modified().unwrap();
            let elapsed_since_modified = now.duration_since(modified_at).unwrap();
            let elapsed_since_indexed = now.duration_since(doc_table.indexed_at).unwrap();

            Some(elapsed_since_indexed > elapsed_since_modified)
        }
        None => None,
    }
}

fn index_doc_by_extension(model: &mut models::Model, doc: &str) -> io::Result<DocHandler> {
    let doc_extension = Path::new(&doc).extension();
    match doc_extension {
        // maybe we should try some threading here
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
            "html" => match parsers::index_html_document(model, doc) {
                Ok(()) => Ok(DocHandler::Indexed),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    Err(err)
                }
            },

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

fn main() -> io::Result<()> {
    match entry() {
        Ok(Some(val)) => match val {
            Commands::Index {
                files_dir,
                index_path,
            } => {
                let files_dir = PathBuf::from(files_dir);
                let docs = read_files_recursively(&files_dir)?;
                let index_table = match get_index_table(&index_path) {
                    Ok(val) => val,
                    Err(_) => models::IndexTable::new(),
                };
                let mut model = models::Model::new(index_table);
                let mut indexed_docs = 0;
                let mut skipped = 0;

                'classify: for doc in docs {
                    // check if document index exists in the index_table;
                    // if it exixts, check whether the file has been modified
                    // since the last index
                    // if yes then reindex the file
                    // if no then skip the file
                    if let Some(is_expired) = doc_index_is_expired(&doc, &model.index_table) {
                        if !is_expired {
                            println!("Skipped {doc}");
                            skipped += 1;
                            continue 'classify;
                        }
                    }

                    //match the document's file extension and index it accordingly
                    match index_doc_by_extension(&mut model, &doc) {
                        Ok(DocHandler::Skipped) => skipped += 1,
                        Ok(DocHandler::Indexed) => indexed_docs += 1,
                        Err(e) => eprintln!("{e}"),
                    }
                }

                // update the models idf
                model.update_idf();

                // write the documents index_table in the provided file path

                println!("Indexed {indexed_docs} documents!");
                println!("Skipped {skipped} documents!");
                println!("Writing into {index_path}...");
                let file = BufWriter::new(File::create(index_path)?);
                serde_json::to_writer(file, &model.index_table)?;
            }
            Commands::Query { index_file, term } => {
                let term_matches = match search_term(&term, &index_file) {
                    Ok(val) => val,
                    Err(err) => {
                        return Err(err);
                    }
                };

                if term_matches.is_empty() {
                    println!("No Matches!");
                    return Ok(());
                }

                for m in term_matches.iter() {
                    println!("{m}");
                }
            }
            Commands::Serve { index_file, port } => {
                server::run_server(&index_file, port);
                return Ok(());
            }
            Commands::Help => {
                usage();
                return Ok(());
            }
            Commands::Version => {
                version_info();
                return Ok(());
            }
        },
        Ok(None) => return Ok(()),
        Err(()) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing Some Arguments",
            ));
        }
    };
    Ok(())
}
