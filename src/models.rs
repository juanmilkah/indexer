use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug)]
/// A Representation of all the indexed documents
pub struct IndexTable {
    /// The number of documents in the index Table
    pub docs_count: u64,
    /// A HashMap of the individual document indexes
    pub tables: FxHashMap<PathBuf, DocTable>,
}

#[derive(Serialize, Deserialize, Debug)]
/// Individual document index
pub struct DocTable {
    /// Time the document was last indexed
    pub indexed_at: SystemTime,
    /// Number of words in the document after stemming
    pub word_count: u64,
    /// A hashmap of words and their count in the document
    pub doc_index: DocIndex,
}

pub type DocIndex = FxHashMap<String, f64>;

#[derive(Serialize, Deserialize)]
/// Document Model, documents => metadata
pub struct Model {
    pub index_table: IndexTable,
}

impl Model {
    pub fn new(index_table: IndexTable) -> Self {
        Self { index_table }
    }

    pub fn add_document(&mut self, doc: &Path, tokens: &[String]) {
        let mut doc_index = DocIndex::default();
        let word_count = tokens.len();

        //calculate term frequencies
        for token in tokens {
            let token = token.trim().to_string();
            let t_count = doc_index.entry(token).or_insert(1.0);
            *t_count += 1.0;
        }

        // convert counts to Tf
        for count in doc_index.values_mut() {
            *count = *count / word_count as f64;
        }

        if !doc_index.is_empty() {
            let doc_table = DocTable {
                indexed_at: SystemTime::now(),
                word_count: word_count as u64,
                doc_index,
            };
            self.index_table.tables.insert(doc.to_path_buf(), doc_table);
            self.index_table.docs_count += 1;
        }
    }

    pub fn update_idf(&mut self) {
        let docs_count = self.index_table.docs_count as f64;
        let mut term_doc_freq = FxHashMap::default();

        //calculate the document freq for each term
        for doc_table in self.index_table.tables.values_mut() {
            for term in doc_table.doc_index.keys() {
                *term_doc_freq.entry(term.clone()).or_insert(0.0) += 1.0;
            }
        }

        // update the tf-idf scores in parallel
        self.index_table
            .tables
            .par_iter_mut()
            .for_each(|(_, doc_table)| {
                for (term, tf) in doc_table.doc_index.iter_mut() {
                    let doc_freq = *term_doc_freq.get(term).unwrap_or(&1.0) as f64;
                    //if the doc_count is 0, add 1_f64 to offset the idf value,
                    // otherwise the idf may be 0 which then makes the tf 0
                    // you get the idea
                    let idf: f64 = (docs_count / doc_freq).ln().abs() + 1_f64;
                    *tf *= idf;
                }
            });
    }

    pub fn search_terms(&self, tokens: &[String]) -> Vec<PathBuf> {
        let mut results = Vec::new();

        for (doc_id, doc_table) in self.index_table.tables.iter() {
            let mut doc_score = 0.0;

            for term in tokens {
                if let Some(score) = doc_table.doc_index.get(term) {
                    doc_score += score;
                }
            }

            if doc_score > 0.0 {
                results.push((doc_id, doc_score));
            }
        }

        //sort results in descending order 9,8,7,6,5,4,3,2,1
        let mut sorted_results: Vec<_> = results.into_iter().collect();
        sorted_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        sorted_results
            .iter()
            .map(|(doc_id, _)| doc_id.to_path_buf())
            .collect::<Vec<PathBuf>>()
    }
}
