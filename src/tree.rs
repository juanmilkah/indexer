use anyhow::Context;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicU64,
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

type DocId = u64;
type TermFrequency = u32;
type Term = String;

#[derive(Serialize, Deserialize, Default)]
pub struct DocumentStore {
    pub doc_to_id: HashMap<PathBuf, DocId>,
    pub id_to_doc_info: HashMap<DocId, DocInfo>,
    pub next_id: AtomicU64,
    pub doc_count: u64, // total number of documents added
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DocInfo {
    pub path: PathBuf,
    pub indexed_at: SystemTime,
}

impl Default for DocInfo {
    fn default() -> Self {
        Self {
            path: Default::default(),
            indexed_at: SystemTime::UNIX_EPOCH,
        }
    }
}
impl DocumentStore {
    pub fn get_id(&mut self, path: &Path) -> DocId {
        if let Some(id) = self.doc_to_id.get(path) {
            *id
        } else {
            let id = self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let doc = path.to_path_buf();
            self.doc_to_id.insert(doc.clone(), id);
            self.id_to_doc_info.insert(
                id,
                DocInfo {
                    path: doc,
                    indexed_at: SystemTime::UNIX_EPOCH,
                },
            );
            self.doc_count += 1;
            id
        }
    }

    fn get_path(&self, id: DocId) -> Option<&PathBuf> {
        self.id_to_doc_info.get(&id).map(|info| &info.path)
    }

    fn total_docs(&self) -> u64 {
        self.doc_count
    }
}

#[derive(Serialize, Deserialize)]
pub struct Posting {
    pub doc_id: DocId,     // The ID of a document containing the term.
    pub tf: TermFrequency, // How many times the term appears in that document.
}

// Metadata for a term within a specific segment's dictionary
#[derive(Serialize, Deserialize, Clone, Copy)]
struct TermInfo {
    df: u32,              // how many docs contain this term
    postings_offset: u64, // byte offset to start position in postings list
    postings_len: u64,    // number of bytes in postings list
}

type SegmentTermInfo = HashMap<Term, TermInfo>;

#[derive(Default)]
pub struct InMemorySegment {
    // Term -> List of postings for docs added to *this segment*
    pub postings: HashMap<Term, Vec<Posting>>,
    pub doc_count: u64, // Number of docs added to this segment
}

impl InMemorySegment {
    fn add_doc(&mut self, doc_id: DocId, terms: &[Term]) {
        self.doc_count += 1;
        let mut term_counts = HashMap::new();

        for term in terms {
            *term_counts.entry(term).or_insert(0) += 1;
        }

        for (term, count) in term_counts {
            self.postings
                .entry((&term).to_string())
                .or_default()
                .push(Posting { doc_id, tf: count });
        }
    }

    // should flush to disk
    fn should_flush(&self, max_docs: u64) -> bool {
        self.doc_count >= max_docs
    }
}

fn flush_segment(
    segment_id: u64,
    segment: &mut InMemorySegment,
    index_dir: &Path,
) -> anyhow::Result<()> {
    if segment.postings.is_empty() {
        return Ok(());
    }

    let segment_dir = index_dir.join(format!("segment_{segment_id}"));
    fs::create_dir_all(&segment_dir).context("create segment dir")?;
    let dict_path = segment_dir.join("term.dict");
    let postings_path = segment_dir.join("postings.bin");

    let mut segment_dict = SegmentTermInfo::new();
    let mut post_writer =
        BufWriter::new(File::create(postings_path).context("create postings file")?);
    let mut current_offset: u64 = 0;

    // Iterate through terms alphabetically for potential locality benefits
    let mut sorted_terms: Vec<_> = segment.postings.keys().cloned().collect();
    sorted_terms.sort();

    for term in sorted_terms {
        if let Some(postings) = segment.postings.get_mut(&term) {
            postings.sort_unstable_by_key(|p| p.doc_id);
            let doc_freq = postings.len() as u32;

            // serialization
            // TODO: apply delta + variable-byte encoding here before writing
            let serialised = bincode2::serialize(postings).context("serialize postings")?;

            let postings_len_bytes = serialised.len() as u64;
            post_writer
                .write_all(&serialised)
                .context("write serialised postings")?;

            segment_dict.insert(
                term.clone(),
                TermInfo {
                    df: doc_freq,
                    postings_offset: current_offset,
                    postings_len: postings_len_bytes,
                },
            );

            current_offset += postings_len_bytes;
        }
    }

    post_writer.flush().context("flush postings writer")?;
    let mut dict_writer = BufWriter::new(File::create(dict_path).context("create dict path")?);
    bincode2::serialize_into(&mut dict_writer, &segment_dict)
        .context("write segment dict into file")?;
    dict_writer.flush().context("flush dict writer")?;

    segment.postings.clear();
    segment.doc_count = 0;

    println!("Flushed segment_{segment_id}");
    Ok(())
}

pub struct MainIndex {
    pub index_dir: PathBuf,
    pub doc_store: DocumentStore,
    pub active_segments: Vec<u64>, // by their ids
    pub current_segment: InMemorySegment,
    pub next_segment: u64,
    pub max_segment_docs: u64,
}

const MAX_SEGMENT_DOCS: u64 = 100;

impl MainIndex {
    pub fn new(index_dir: &Path) -> anyhow::Result<Self> {
        let docstore_filepath = index_dir.join("docstore.bin");

        let doc_store: DocumentStore = {
            match File::open(docstore_filepath) {
                Ok(f) => {
                    let mut reader = BufReader::new(f);

                    bincode2::deserialize_from(&mut reader).unwrap_or_default()
                }
                Err(_) => DocumentStore::default(),
            }
        };

        let paths: Vec<PathBuf> = match fs::read_dir(index_dir) {
            Ok(values) => values.map(|e| e.unwrap().path().to_path_buf()).collect(),
            Err(_) => Vec::new(),
        };

        let mut segments = Vec::new();
        for path in paths {
            if path.is_dir()
                && path.to_string_lossy().to_string().contains("segment_")
                && let Some(prefix) = path.file_stem()
            {
                let name = prefix.to_string_lossy().to_string();
                let (_, seg_id) = name.split_once("segment_").unwrap();
                let seg_id = seg_id
                    .to_string()
                    .parse::<u64>()
                    .context("parsing segment id")?;
                segments.push(seg_id);
            }
        }

        let next_segment = segments.iter().max().cloned().unwrap_or(0) + 1;

        Ok(Self {
            index_dir: index_dir.to_path_buf(),
            doc_store,
            active_segments: segments,
            current_segment: InMemorySegment::default(),
            next_segment,
            max_segment_docs: MAX_SEGMENT_DOCS,
        })
    }

    pub fn add_document(&mut self, doc_path: &Path, terms: &[Term]) -> anyhow::Result<()> {
        if terms.is_empty() {
            return Ok(());
        }

        let doc_id = self.doc_store.get_id(doc_path);
        self.current_segment.add_doc(doc_id, terms);
        if let Some(doc_info) = self.doc_store.id_to_doc_info.get_mut(&doc_id) {
            doc_info.indexed_at = SystemTime::now();
        }

        if self.current_segment.should_flush(self.max_segment_docs) {
            let seg_id = self.next_segment;
            flush_segment(seg_id, &mut self.current_segment, &self.index_dir)
                .context("flush segment")?;
            self.next_segment += 1;
            self.active_segments.push(seg_id);
        }

        Ok(())
    }

    // flush the last partially filled segment
    pub fn commit(&mut self) -> anyhow::Result<()> {
        if self.current_segment.doc_count > 0 {
            let seg_id = self.next_segment;
            flush_segment(seg_id, &mut self.current_segment, &self.index_dir)
                .context("flush partially filled")?;
            self.active_segments.push(seg_id);
            self.next_segment += 1;
        }

        let mut writer = BufWriter::new(
            File::create(self.index_dir.join("docstore.bin")).context("create docstore")?,
        );
        bincode2::serialize_into(&mut writer, &self.doc_store)
            .context("serialize doc store into file")?;
        Ok(())
    }

    pub fn search(&self, q_tokens: &[Term]) -> anyhow::Result<Vec<(PathBuf, f64)>> {
        let mut scores: HashMap<DocId, f64> = HashMap::new();
        let total_docs = self.doc_store.total_docs();

        let mut terms_info_cache: HashMap<Term, Vec<(DocId, TermInfo)>> = HashMap::new();
        let mut global_dfs: HashMap<Term, u32> = HashMap::new();

        // Pass 1: Load dictionaries and calculate global DFs
        for &seg_id in &self.active_segments {
            let dict_path = self
                .index_dir
                .join(format!("segment_{seg_id}"))
                .join("term.dict");
            let mut reader = BufReader::new(File::open(dict_path).context("open dict path")?);

            let seg_dict: SegmentTermInfo =
                bincode2::deserialize_from(&mut reader).context("deserialise seg dict")?;

            for token in q_tokens {
                if let Some(metadata) = seg_dict.get(token) {
                    terms_info_cache
                        .entry(token.to_string())
                        .or_default()
                        .push((seg_id, *metadata));

                    *global_dfs.entry(token.to_string()).or_insert(0) += metadata.df;
                }
            }
        }

        // Pass 2: Read postings and calculate scores
        for token in q_tokens {
            let global_df = global_dfs.get(token).cloned().unwrap_or(0) as f64;
            if global_df == 0.0 {
                continue;
            }

            let idf = (total_docs as f64 / global_df).ln().abs();

            if let Some(postings_hit) = terms_info_cache.get(token) {
                for (seg_id, metadata) in postings_hit {
                    let posting_path = self
                        .index_dir
                        .join(format!("segment_{seg_id}"))
                        .join("postings.bin");
                    let mut reader =
                        BufReader::new(File::open(&posting_path).context("open postings path")?);

                    reader
                        .seek(SeekFrom::Start(metadata.postings_offset))
                        .context("seek to postings offset")?;
                    let mut reader = reader.take(metadata.postings_len);

                    let deserialised: Vec<Posting> = bincode2::deserialize_from(&mut reader)
                        .context("deserialise from post reader")?;

                    for posting in deserialised {
                        let tf = posting.tf as f64;
                        let tf_idf = tf * idf;
                        *scores.entry(posting.doc_id).or_insert(0.0) += tf_idf;
                    }
                }
            }
        }

        let mut results: Vec<(PathBuf, f64)> = Vec::new();
        for (doc_id, score) in scores {
            let path = self.doc_store.get_path(doc_id).unwrap();
            if score != 0.0 {
                results.push((path.clone(), score));
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }
}
