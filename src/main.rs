use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use poppler::PopplerDocument;

enum Commands {
    Search {
        index_file: String,
        term: String,
    },
    Index {
        files_dir: String,
        index_path: String,
    },
}

type DocsIndex = BTreeMap<String, DocIndex>;
type DocIndex = BTreeMap<String, f32>;

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

    fn relation(&self, concodance_1: &DocIndex, concodance_2: &DocIndex) -> f32 {
        let mut top_value: f32 = 0.0;

        for (word, count) in concodance_1.iter() {
            if concodance_2.contains_key(word) {
                top_value += count * concodance_2.get(word).unwrap();
            }
        }

        let conc_1 = self.magnitude(concodance_1);
        let conc_2 = self.magnitude(concodance_2);

        if conc_1 * conc_2 != 0.0 {
            top_value / (conc_1 * conc_2)
        } else {
            0.0
        }
    }
}

struct Lexer {
    input: Vec<char>,
    position: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            input: input.to_lowercase().chars().collect(),
            position: 0,
        }
    }

    fn parse(&mut self) -> Vec<String> {
        let mut tokens = Vec::new();

        while self.position < self.input.len() {
            let current = self.input[self.position];
            let start = self.position;

            if current == '\n' || !current.is_ascii_alphanumeric() {
                self.position += 1;
                continue;
            }

            if current.is_ascii_alphanumeric() {
                while self.position < self.input.len()
                    && self.input[self.position].is_ascii_alphanumeric()
                {
                    self.position += 1;
                }
            } else {
                self.position += 1;
            }

            let token: String = self.input[start..self.position].iter().collect();

            if !token.is_empty() {
                tokens.push(token);
            }
        }

        tokens
    }

    fn remove_stop_words(&self, tokens: &[String]) -> Vec<String> {
        let stop_words = stop_words::get(stop_words::LANGUAGE::English);
        let mut cleaned = Vec::new();

        for token in tokens {
            if stop_words.contains(token) {
                continue;
            }
            cleaned.push(token.to_string());
        }

        cleaned
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

                if index_documents(&v, &docs, &index_path).is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Failed to build Index",
                    ));
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
                    println!("{}: \t{}", m.0, m.1);
                }
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

fn entry() -> Result<Option<Commands>, ()> {
    let mut args = env::args().skip(1).peekable();

    if let Some(subcommand) = args.next() {
        match subcommand.as_str() {
            "search" => {
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
            "index" => {
                if let Some(dir) = args.next() {
                    if let Some(index) = args.next() {
                        Ok(Some(Commands::Index {
                            files_dir: dir,
                            index_path: index,
                        }))
                    } else {
                        usage();
                        Ok(None)
                    }
                } else {
                    usage();
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    } else {
        usage();
        return Ok(None);
    }
}

fn usage() {
    println!("USAGE: [PROGRAM] [SUBCOMMAND] [OPTIONS]");
    println!("SubCommands:");
    println!("\tsearch <index_path> <term>       Search for a term in documents");
    println!("\tindex <directory> <index_path>   Create an index from a directory");
}

fn index_documents(v: &VectorCompare, docs: &[String], index_path: &str) -> io::Result<()> {
    //build the index
    let mut docs_index = BTreeMap::new();

    for doc in docs.iter() {
        println!("Indexing document: {doc}");
        let document = match PopplerDocument::new_from_file(doc, None) {
            Ok(doc) => doc,
            Err(err) => {
                eprintln!("Failed to load document: {err}");
                continue;
            }
        };

        let mut doc_index = BTreeMap::new();
        let end = document.get_n_pages();

        for i in 1..end {
            if let Some(page) = document.get_page(i) {
                if let Some(text) = page.get_text() {
                    let mut lex = Lexer::new(text);
                    let tokens = lex.parse();
                    let tokens = lex.remove_stop_words(&tokens);
                    doc_index = v.concodance(&tokens, &mut doc_index);
                }
            }
        }

        docs_index.insert(doc.clone(), doc_index);
    }
    println!("Completed Indexing!");

    println!("Writing into {index_path}...");
    let file = BufWriter::new(File::create(index_path)?);
    serde_json::to_writer(file, &docs_index)?;
    Ok(())
}

fn search_term(v: &VectorCompare, term: &str, index_file: &str) -> io::Result<Vec<(f32, String)>> {
    let mut matches = Vec::new();

    let file = BufReader::new(File::open(index_file)?);
    let docs_index: DocsIndex = serde_json::from_reader(file)?;

    let tokens = Lexer::new(term).parse();
    let term_conc = v.concodance(&tokens, &mut BTreeMap::new());
    for (doc_name, doc_index) in docs_index.iter() {
        let relation = v.relation(&term_conc, doc_index);

        if relation != 0.0 {
            // relation = relation.log10().abs();
            matches.push((relation, doc_name.clone()));
        }
    }

    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    matches.reverse();
    Ok(matches)
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
