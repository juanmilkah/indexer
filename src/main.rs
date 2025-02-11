use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{char, env};

use home::home_dir;
use poppler::PopplerDocument;
use rust_stemmers::{Algorithm, Stemmer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_http::{Method, Response, Server};

enum Commands {
    Search {
        index_file: String,
        term: String,
    },
    Index {
        files_dir: String,
        index_path: String,
    },
    Serve {
        index_file: String,
    },
    Help,
    Version,
}

type DocIndex = HashMap<String, f32>;

#[derive(Serialize, Deserialize)]
struct IndexTable {
    docs_count: u64,
    tables: HashMap<String, DocTable>,
}

#[derive(Serialize, Deserialize)]
struct DocTable {
    indexed_at: SystemTime,
    word_count: u64,
    doc_index: DocIndex,
}

impl IndexTable {
    fn new() -> Self {
        Self {
            docs_count: 0,
            tables: HashMap::new(),
        }
    }
}

struct VectorCompare;

impl VectorCompare {
    // count of every word that occurs in a document
    fn concodance(&self, tokens: &[String], map: &mut DocIndex) -> DocIndex {
        for token in tokens {
            let token = token.trim_ascii();
            let mut count: f32 = *map.entry(token.to_string()).or_insert(0.0);
            count += 1.0;

            map.insert(token.to_string(), count);
        }

        map.clone()
    }

    fn magnitude(&self, concodance: &DocIndex) -> f32 {
        let mut total = 0.0;

        for (_, count) in concodance.iter() {
            total += count * count;
        }

        total.sqrt()
    }

    fn relation(&self, minor_doc: &DocIndex, major_doc: &DocIndex) -> f32 {
        let mut top_value: f32 = 0.0;

        for (word, count) in minor_doc.iter() {
            if major_doc.contains_key(word) {
                top_value += count * major_doc.get(word).unwrap();
            }
        }

        let conc_1 = self.magnitude(minor_doc);
        let conc_2 = self.magnitude(major_doc);

        if conc_1 * conc_2 != 0.0 {
            top_value / (conc_1 * conc_2)
        } else {
            0.0
        }
    }
}

struct Lexer<'a> {
    input: &'a [char],
}

impl<'a> Lexer<'a> {
    fn new(input: &'a [char]) -> Self {
        Self { input }
    }

    fn trim_left(&mut self) {
        while !self.input.is_empty() && self.input[0].is_whitespace() {
            self.input = &self.input[1..];
        }
    }

    fn chop(&mut self, n: usize) -> &'a [char] {
        let token = &self.input[0..n];
        self.input = &self.input[n..];
        token
    }

    fn chop_while<P>(&mut self, mut predicate: P) -> &'a [char]
    where
        P: FnMut(&char) -> bool,
    {
        let mut n = 0;
        while n < self.input.len() && predicate(&self.input[n]) {
            n += 1;
        }

        self.chop(n)
    }

    fn next_token(&mut self) -> Option<String> {
        self.trim_left();

        if self.input.is_empty() {
            return None;
        }

        if self.input[0].is_numeric() {
            return Some(self.chop_while(|x| x.is_numeric()).iter().collect());
        }

        if self.input[0].is_alphabetic() {
            let term: String = self.chop_while(|x| x.is_alphanumeric()).iter().collect();

            let stemmed_token = self.stem_token(&term);
            return Some(stemmed_token);
        }
        Some(self.chop(1).iter().collect())
    }

    fn stem_token(&self, token: &str) -> String {
        let stemmer = Stemmer::create(Algorithm::English);
        stemmer.stem(token).to_string()
    }

    fn remove_stop_words(&self, tokens: &[String]) -> Vec<String> {
        let mut set = HashSet::new();
        let _ = stop_words::get(stop_words::LANGUAGE::English)
            .into_iter()
            .map(|w| set.insert(w));
        let mut cleaned = Vec::new();

        for token in tokens {
            if set.contains(token) {
                continue;
            }
            cleaned.push(token.to_string());
        }

        cleaned
    }
}

impl Iterator for Lexer<'_> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

fn entry() -> Result<Option<Commands>, ()> {
    let mut args = env::args().skip(1).peekable();

    if let Some(subcommand) = args.next() {
        match subcommand.as_str() {
            "search" | "-q" => {
                if let Some(index_file) = args.next() {
                    if let Some(term) = args.next() {
                        Ok(Some(Commands::Search { term, index_file }))
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
            //fix this later
            "serve" | "-s" => {
                if let Some(index_file) = args.next() {
                    Ok(Some(Commands::Serve { index_file }))
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
    println!("USAGE: [PROGRAM] [COMMANDS] [OPTIONS]");
    println!("SubCommands:");
    println!("\tsearch <index_path> <term>       Search for a term in documents");
    println!("\tindex <directory> [index_path]   Create an index from a directory");
    println!("\tserve <index_path>               Serve the responses to an http server");
}

fn index_pdf_document(v: &VectorCompare, filepath: &str) -> io::Result<DocTable> {
    println!("Indexing document: {filepath}");
    let indexed_at = SystemTime::now();
    let document = match PopplerDocument::new_from_file(filepath, None) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("Failed to load document: {err}");
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{err:?}"),
            ));
        }
    };

    let mut doc_index: DocIndex = HashMap::new();
    let end = document.get_n_pages();

    for i in 1..end {
        if let Some(page) = document.get_page(i) {
            if let Some(text) = page.get_text() {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut tokens = Vec::new();
                {
                    let mut lex = Lexer::new(&text_chars);

                    while let Some(token) = lex.next() {
                        let token = lex.stem_token(&token);
                        tokens.push(token);
                    }

                    tokens = lex.remove_stop_words(&tokens);
                }
                doc_index = v.concodance(&tokens, &mut doc_index);
            }
        }
    }
    let doc_table = DocTable {
        indexed_at,
        word_count: doc_index.keys().len() as u64,
        doc_index,
    };

    Ok(doc_table)
}

fn index_text_document(v: &VectorCompare, filepath: &str) -> io::Result<DocTable> {
    println!("Indexing {filepath}...");
    let indexed_at = SystemTime::now();
    let content = match fs::read_to_string(filepath) {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Failed to read file {filepath}: {err}");
            return Err(err);
        }
    };

    let content = content.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&content);
    let mut tokens = Vec::new();
    while let Some(token) = lex.next_token() {
        tokens.push(token);
    }

    let tokens = lex.remove_stop_words(&tokens);
    let doc_index = v.concodance(&tokens, &mut HashMap::new());

    Ok(DocTable {
        indexed_at,
        word_count: doc_index.len() as u64,
        doc_index,
    })
}

fn search_term(v: &VectorCompare, term: &str, index_file: &str) -> io::Result<Vec<String>> {
    let mut matches = Vec::new();

    let file = BufReader::new(File::open(index_file)?);
    let index_table: IndexTable = serde_json::from_reader(file)?;

    let text_chars = term.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&text_chars);

    let mut tokens = Vec::new();

    while let Some(token) = lex.next() {
        let token = lex.stem_token(&token);
        tokens.push(token);
    }

    let tokens = lex.remove_stop_words(&tokens);

    let term_conc = v.concodance(&tokens, &mut HashMap::new());
    for (doc_name, doc_table) in index_table.tables.iter() {
        let relation = v.relation(&term_conc, &doc_table.doc_index);

        if relation != 0.0 {
            // relation = relation.log10().abs();
            matches.push((relation, doc_name.clone()));
        }
    }

    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    matches.reverse();
    let result = matches.iter().map(|v| v.1.clone()).collect::<Vec<String>>();
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

//fix this -->figure out how to read from the Cargo.toml file
fn version_info() {
    println!("INDEXER VERSION 0.1.0");
}

fn get_index_table(filepath: &str) -> io::Result<IndexTable> {
    let index_file = File::open(filepath)?;
    let index_table: IndexTable = serde_json::from_reader(&index_file)?;
    Ok(index_table)
}

fn run_server(index_file: &str) {
    let port = "0.0.0.0:8080";
    let server = match Server::http(port) {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Failed to bind server to port {port}: {err}");
            return;
        }
    };
    println!("Server listening on port {port}");

    for mut request in server.incoming_requests() {
        match &request.method() {
            Method::Get => match request.url() {
                "/" => {
                    // respond with the index.html file
                    let html_file = home_dir()
                        .unwrap_or(PathBuf::from("."))
                        .join(".indexer")
                        .join("index.html")
                        .to_string_lossy()
                        .to_string();
                    let response = Response::from_file(File::open(&html_file).unwrap());
                    let _ = request.respond(response);
                }
                _ => {
                    let response = Response::from_string(format!(
                        "Route not Allowed: {url}",
                        url = request.url()
                    ));
                    let _ = request.respond(response.with_status_code(403));
                }
            },
            Method::Post => match request.url() {
                "/search" => {
                    let v = VectorCompare;
                    let mut body = String::new();
                    let _ = &request.as_reader().read_to_string(&mut body);
                    let json: Value = body.parse().unwrap_or_default();

                    if let Some(query) = json.get("query") {
                        let query = query.to_string();

                        match search_term(&v, &query, index_file) {
                            Ok(vals) => {
                                let response = Response::from_string(vals.join("\n"));
                                let _ = request.respond(response);
                            }
                            Err(err) => {
                                let response = Response::from_string(format!(
                                    "Failed to search for query: {err}"
                                ));
                                let _ = request.respond(response.with_status_code(500));
                            }
                        };
                    } else {
                        let response = Response::from_string("Failed to parse request body");
                        let _ = request.respond(response.with_status_code(500));
                    }
                }
                _ => {
                    let response = Response::from_string(format!(
                        "Route not Allowed: {url}",
                        url = request.url()
                    ));
                    let _ = request.respond(response.with_status_code(403));
                }
            },
            _ => {
                let response = Response::from_string(format!(
                    "Method Not Allowed: {method}",
                    method = request.method()
                ));
                let _ = request.respond(response.with_status_code(403));
            }
        }
    }
}

fn doc_index_is_expired(doc: &str, index_table: &IndexTable) -> Option<bool> {
    let now = SystemTime::now();
    match index_table.tables.get(doc) {
        Some(doc_table) => {
            let modified_at = Path::new(&doc).metadata().unwrap().modified().unwrap();
            let elapsed_since_modified = now.duration_since(modified_at).unwrap();
            let elapsed_since_indexed = now.duration_since(doc_table.indexed_at).unwrap();

            Some(elapsed_since_indexed > elapsed_since_modified)
        }
        None => None,
    }
}

fn index_doc_by_extension(v: &VectorCompare, doc: &str) -> Option<DocTable> {
    let doc_extension = Path::new(&doc).extension();
    match doc_extension {
        Some(ext) => match ext.to_str().unwrap() {
            "pdf" => match index_pdf_document(v, doc) {
                Ok(doc_table) => Some(doc_table),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    None
                }
            },
            "txt" | "md" => match index_text_document(v, doc) {
                Ok(doc_table) => Some(doc_table),
                Err(err) => {
                    eprintln!("Failed to index {doc}: {err}");
                    None
                }
            },
            _ => None,
        },
        None => None,
    }
}

fn main() -> io::Result<()> {
    let v = VectorCompare;
    match entry() {
        Ok(Some(val)) => match val {
            Commands::Index {
                files_dir,
                index_path,
            } => {
                let files_dir = PathBuf::from(files_dir);
                let docs = read_files_recursively(&files_dir)?;
                let mut index_table = match get_index_table(&index_path) {
                    Ok(val) => val,
                    Err(_) => IndexTable::new(),
                };

                let mut counter: usize = 0;

                for doc in docs {
                    // check if document index exists in the index_table;
                    // if it exixts, check whether the file has been modified
                    // since the last index
                    // if yes then reindex the file
                    // if no then skip the file
                    if let Some(is_expired) = doc_index_is_expired(&doc, &index_table) {
                        if !is_expired {
                            continue;
                        }
                    }

                    //match the document's file extension and index it accordingly
                    if let Some(doc_table) = index_doc_by_extension(&v, &doc) {
                        index_table.tables.insert(doc, doc_table);
                        counter += 1;
                    }
                }

                // write the documents index_table in the provided file path
                println!(
                    "Indexed {counter} new document{}!",
                    if counter == 1 { "" } else { "s" }
                );

                if counter != 0 {
                    println!("Writing into {index_path}...");
                    let file = BufWriter::new(File::create(index_path)?);
                    serde_json::to_writer(file, &index_table)?;
                }
            }
            Commands::Search { index_file, term } => {
                let term_matches = match search_term(&v, &term, &index_file) {
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
            Commands::Serve { index_file } => {
                run_server(&index_file);
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
