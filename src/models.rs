use std::collections::HashMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
/// A Representation of all the indexed documents
pub struct IndexTable {
    /// The number of documents in the index Table
    pub docs_count: u64,
    /// A Hashmap of the individual document indexes
    pub tables: HashMap<String, DocTable>,
}

#[derive(Serialize, Deserialize)]
/// Individual document index
pub struct DocTable {
    /// Time the document was last indexed
    pub indexed_at: SystemTime,
    /// Number of words in the document after stemming
    pub word_count: u64,
    /// A hashmap of words and their count in the document
    pub doc_index: DocIndex,
}

pub type DocIndex = HashMap<String, f32>;

impl IndexTable {
    pub fn new() -> Self {
        Self {
            docs_count: 0,
            tables: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
/// The Document Model containing the index table for all the documents and their metadata
pub struct Model {
    pub index_table: IndexTable,
}

impl Model {
    pub fn new(index_table: IndexTable) -> Self {
        Self { index_table }
    }

    pub fn add_document(&mut self, doc: &str, tokens: &[String]) {
        let mut doc_index = DocIndex::new();
        let word_count = tokens.len() as f64;

        //calculate term frequencies
        for token in tokens {
            let token = token.trim().to_string();
            let t_count = doc_index.entry(token).or_insert(1.0);
            *t_count += 1.0;
        }

        // convert counts to Tf
        for count in doc_index.values_mut() {
            *count = (*count / word_count as f32) + 1.0_f32;
        }

        if !doc_index.is_empty() {
            let doc_table = DocTable {
                indexed_at: SystemTime::now(),
                word_count: word_count as u64,
                doc_index,
            };
            self.index_table.tables.insert(doc.to_string(), doc_table);
            self.index_table.docs_count += 1;
        }
    }

    pub fn update_idf(&mut self) {
        let docs_count = self.index_table.docs_count as f32;
        let mut term_doc_freq = HashMap::new();

        //calculate the document freq for each term
        for doc_table in self.index_table.tables.values_mut() {
            for term in doc_table.doc_index.keys() {
                *term_doc_freq.entry(term.clone()).or_insert(1.0) += 1.0;
            }
        }

        // update the tf-idf scores
        for doc_table in self.index_table.tables.values_mut() {
            for (term, tf) in doc_table.doc_index.iter_mut() {
                let doc_freq = *term_doc_freq.get(term).unwrap_or(&1.0) as f32;
                let idf: f32 = (docs_count / doc_freq).ln().abs();
                *tf *= idf;
            }
        }
    }

    pub fn search_terms(&self, tokens: &[String]) -> Vec<String> {
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
            .map(|(doc_id, _)| doc_id.to_string())
            .collect::<Vec<String>>()
    }
}
