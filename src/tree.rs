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

/// Type alias for Document ID.
type DocId = u64;
/// Type alias for Term Frequency.
type TermFrequency = u32;
/// Type alias for a search Term.
type Term = String;

/// Stores metadata about documents, mapping paths to IDs and vice-versa.
#[derive(Serialize, Deserialize, Default)]
pub struct DocumentStore {
    /// Maps document paths to their unique IDs.
    pub doc_to_id: HashMap<PathBuf, DocId>,
    /// Maps document IDs to `DocInfo` containing path and indexed time.
    pub id_to_doc_info: HashMap<DocId, DocInfo>,
    /// The next available document ID.
    pub next_id: AtomicU64,
    /// Total number of documents added to the store.
    pub doc_count: u64,
}

/// Contains information about a document, including its path and the time it
/// was indexed.
#[derive(Serialize, Deserialize, Clone)]
pub struct DocInfo {
    /// The file path of the document.
    pub path: PathBuf,
    /// The `SystemTime` when the document was indexed.
    pub indexed_at: SystemTime,
}

impl Default for DocInfo {
    /// Returns a default `DocInfo` with an empty path and `UNIX_EPOCH` for
    /// indexed time.
    fn default() -> Self {
        Self {
            path: Default::default(),
            indexed_at: SystemTime::UNIX_EPOCH,
        }
    }
}

impl DocumentStore {
    /// Retrieves the unique document ID for a given path. If the path is new,
    /// it assigns a new ID and stores the document information.
    ///
    /// # Arguments
    /// * `path` - The `Path` of the document.
    ///
    /// # Returns
    /// The `DocId` for the given document path.
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

    /// Retrieves the `PathBuf` associated with a given `DocId`.
    ///
    /// # Arguments
    /// * `id` - The `DocId` to look up.
    ///
    /// # Returns
    /// An `Option` containing a reference to the `PathBuf` if found, otherwise
    ///  `None`.
    fn get_path(&self, id: DocId) -> Option<&PathBuf> {
        self.id_to_doc_info.get(&id).map(|info| &info.path)
    }

    /// Returns the total number of documents in the store.
    ///
    /// # Returns
    /// The total count of documents as `u64`.
    fn total_docs(&self) -> u64 {
        self.doc_count
    }
}

/// Represents a posting in an inverted index, linking a document ID
/// to the term's frequency within that document.
#[derive(Serialize, Deserialize)]
pub struct Posting {
    /// The ID of a document containing the term.
    pub doc_id: DocId,
    /// How many times the term appears in that document.
    pub tf: TermFrequency,
}

/// Metadata for a term within a specific segment's dictionary.
#[derive(Serialize, Deserialize, Clone, Copy)]
struct TermInfo {
    /// How many documents contain this term within the segment.
    df: u32,
    /// Byte offset to the start position of the postings list for this term in
    ///  the postings file.
    postings_offset: u64,
    /// Number of bytes in the postings list for this term.
    postings_len: u64,
}

/// Type alias for a segment's term information, mapping terms to `TermInfo`.
type SegmentTermInfo = HashMap<Term, TermInfo>;

/// Represents an in-memory segment of the index, holding postings before
/// flushing to disk.
#[derive(Default)]
pub struct InMemorySegment {
    /// Maps terms to a list of postings for documents added to *this segment*.
    pub postings: HashMap<Term, Vec<Posting>>,
    /// Number of documents added to this segment.
    pub doc_count: u64,
}

impl InMemorySegment {
    /// Adds a document and its terms to the in-memory segment.
    ///
    /// # Arguments
    /// * `doc_id` - The ID of the document.
    /// * `terms` - A slice of terms found in the document.
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

    /// Determines if the current in-memory segment should be flushed to disk.
    ///
    /// # Arguments
    /// * `max_docs` - The maximum number of documents allowed in this segment
    ///   before flushing.
    ///
    /// # Returns
    /// `true` if the segment's document count meets or exceeds `max_docs`,
    /// `false` otherwise.
    fn should_flush(&self, max_docs: u64) -> bool {
        self.doc_count >= max_docs
    }
}

/// Flushes the contents of an `InMemorySegment` to disk, creating segment files
/// for the term dictionary and postings lists.
///
/// # Arguments
/// * `segment_id` - The unique ID of the segment being flushed.
/// * `segment` - A mutable reference to the `InMemorySegment` to flush.
/// * `index_dir` - The base directory where index segments are stored.
///
/// # Returns
/// `Ok(())` if the flush was successful, otherwise an `anyhow::Result` error.
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

/// Represents the main inverted index, managing document storage, segments,
/// and search operations.
pub struct MainIndex {
    /// The base directory where all index files and segments are stored.
    pub index_dir: PathBuf,
    /// The store for document metadata.
    pub doc_store: DocumentStore,
    /// A list of active segment IDs.
    pub active_segments: Vec<u64>,
    /// The current in-memory segment being built.
    pub current_segment: InMemorySegment,
    /// The ID for the next segment to be created.
    pub next_segment: u64,
    /// The maximum number of documents an in-memory segment can hold before
    /// being flushed.
    pub max_segment_docs: u64,
}

/// Constant defining the maximum number of documents allowed in an in-memory
/// segment before flushing.
const MAX_SEGMENT_DOCS: u64 = 100;

impl MainIndex {
    /// Creates a new `MainIndex` instance. It loads existing document store
    /// and segments
    /// from the `index_dir` if available, or initializes a new index.
    ///
    /// # Arguments
    /// * `index_dir` - The directory where index files are located or will be
    ///   stored.
    ///
    /// # Returns
    /// `Ok(Self)` if successful, otherwise an `anyhow::Result` error.
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

    /// Adds a document to the index. It tokenizes the document, adds it to the
    /// current in-memory segment, and flushes the segment to disk if it exceeds
    /// `max_segment_docs`.
    ///
    /// # Arguments
    /// * `doc_path` - The path to the document to add.
    /// * `terms` - A slice of terms extracted from the document.
    ///
    /// # Returns
    /// `Ok(())` if the document was added successfully, otherwise an
    /// `anyhow::Result` error.
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

    /// Commits the current state of the index, flushing any partially filled
    /// in-memory segment to disk and saving the `DocumentStore`.
    ///
    /// # Returns
    /// `Ok(())` if the commit was successful, otherwise an `anyhow::Result`
    /// error.
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

    /// Searches the index for documents matching the given query tokens.
    /// It calculates TF-IDF scores for each matching document across all active
    /// segments.
    ///
    /// # Arguments
    /// * `q_tokens` - A slice of terms representing the search query.
    ///
    /// # Returns
    /// A `Vec` of tuples, where each tuple contains the `PathBuf` of a matching
    /// document and its calculated TF-IDF score, sorted in descending order of
    /// score.
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

            // Calculate Inverse Document Frequency (IDF)
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
