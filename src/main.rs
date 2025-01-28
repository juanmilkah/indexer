use std::collections::HashMap;
use std::fs::{self, File};
use std::{
    env,
    io::{self, BufReader, BufWriter},
};

use lopdf::Document;

enum Commands {
    Search { term: String, index_file: String },
    Index { files_dir: String },
}

type DocsIndex = HashMap<String, DocIndex>;
type DocIndex = HashMap<String, f32>;

struct VectorCompare;

impl VectorCompare {
    // count of every word that occurs in a document
    fn concodance(&self, document: String) -> HashMap<String, f32> {
        let mut map = HashMap::new();

        for word in document.split(" ") {
            let mut count: f32 = *map.entry(word.to_string()).or_insert(0.0);
            count += 1.0;

            map.insert(word.to_string(), count);
        }

        map
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

fn main() -> io::Result<()> {
    let v = VectorCompare;
    match entry() {
        Ok(val) => match val {
            Commands::Index { files_dir } => {
                let docs = fs::read_dir(&files_dir)
                    .unwrap()
                    .map(|entry| entry.unwrap().path().to_string_lossy().to_string())
                    .collect::<Vec<String>>();

                if index_documents(&v, &docs).is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Failed to build Index",
                    ));
                }
            }
            Commands::Search { term, index_file } => {
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

                for m in term_matches.iter().take(10) {
                    println!("{}: \t{}", m.0, m.1);
                }
            }
        },
        Err(err) => return Err(err),
    };

    Ok(())
}

fn index_documents(v: &VectorCompare, docs: &[String]) -> io::Result<()> {
    //build the index
    let mut index = HashMap::new();

    for doc in docs.iter() {
        println!("Indexing document: {doc}");
        let document = match Document::load(doc) {
            Ok(doc) => doc,
            Err(err) => {
                eprintln!("Failed to load document: {err}");
                continue;
            }
        };
        let text = match document.extract_text(&[1]) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("Failed to extract text from document: {err}");
                continue;
            }
        };

        let conc = v.concodance(text.to_lowercase());
        index.insert(doc.clone(), conc);
    }
    println!("Completed Indexing!");

    let file = BufWriter::new(File::create("index.json")?);
    println!("Writing into Index.json...");

    serde_json::to_writer(file, &index)?;
    Ok(())
}

fn search_term(v: &VectorCompare, term: &str, index_file: &str) -> io::Result<Vec<(f32, String)>> {
    let mut matches = Vec::new();

    let file = BufReader::new(File::open(index_file)?);
    let docs_index: DocsIndex = serde_json::from_reader(file)?;

    let term_conc = v.concodance(term.to_string());
    for (doc_name, doc_index) in docs_index.iter() {
        let relation = v.relation(&term_conc, doc_index);

        if relation != 0.0 {
            matches.push((relation, doc_name.clone()));
        }
    }

    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    matches.reverse();
    Ok(matches)
}

fn entry() -> io::Result<Commands> {
    let mut args = env::args();

    if args.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing some Arguments",
        ));
    }
    args.next().unwrap(); //program

    match args.next().unwrap().as_str() {
        "search" => {
            let index_file = match args.next() {
                Some(file) => file,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Missing index file",
                    ));
                }
            };

            let term = match args.next() {
                Some(t) => t,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Missing index file",
                    ));
                }
            };

            Ok(Commands::Search { term, index_file })
        }
        "index" => {
            let index_file = match args.next() {
                Some(file) => file,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Missing index file",
                    ));
                }
            };

            Ok(Commands::Index {
                files_dir: index_file,
            })
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing some Arguments",
        )),
    }
}
