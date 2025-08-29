use anyhow::Context;
use html5ever::driver::{self, ParseOpts};
use lopdf;
use scraper::{Html, HtmlTreeSink};
use tendril::TendrilSink;
use xml::EventReader;
use xml::reader::XmlEvent;

use crate::Message;
use crate::lexer::Lexer;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, RwLock, mpsc};

/// Parses a CSV document, extracts text content from all fields, tokenizes it,
/// and removes stop words.
///
/// # Arguments
/// * `filepath` - The path to the CSV file.
/// * `err_handler` - A sender for logging messages.
/// * `stop_words` - A slice of stop words to filter out.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of processed tokens on success, or an
/// `anyhow::Error` on failure.
pub fn parse_csv_document(
    filepath: &Path,
    err_handler: Arc<RwLock<mpsc::Sender<Message>>>,
    stop_words: &[String],
) -> Vec<String> {
    {
        let _ = err_handler
            .read()
            .unwrap()
            .send(Message::Info(format!("Indexing document: {filepath:?}")));
    }

    let f = match File::open(filepath).context("open filepath") {
        Ok(f) => f,
        Err(err) => {
            let _ = err_handler
                .read()
                .unwrap()
                .send(Message::Error(format!("{err}")));
            return Vec::new();
        }
    };
    let reader = BufReader::new(f);
    let mut rdr = csv::Reader::from_reader(reader);

    let mut fields = String::new();

    for record in rdr.records() {
        // The iterator yields Result<StringRecord, Error>
        let record = match record {
            Ok(r) => r,
            Err(_) => continue,
        };
        for field in record.iter() {
            fields.push_str(field);
        }
    }

    let fields_chars = fields.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&fields_chars);
    let tokens = lex.get_tokens(stop_words);
    tokens
}

/// Parses an HTML document, extracts all visible text content, tokenizes it,
/// and removes stop words.
///
/// # Arguments
/// * `filepath` - The path to the HTML file.
/// * `err_handler` - A sender for logging messages.
/// * `stop_words` - A slice of stop words to filter out.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of processed tokens on success, or an
/// `anyhow::Error` on failure.
pub fn parse_html_document(
    filepath: &Path,
    err_handler: Arc<RwLock<mpsc::Sender<Message>>>,
    stop_words: &[String],
) -> Vec<String> {
    {
        let _ = err_handler
            .read()
            .unwrap()
            .send(Message::Info(format!("Indexing document: {filepath:?}")));
    }
    let document = match fs::read_to_string(filepath) {
        Ok(c) => c,
        Err(err) => {
            let _ = err_handler
                .read()
                .unwrap()
                .send(Message::Error(format!("{err}")));
            return Vec::new();
        }
    };
    let parser = driver::parse_document(
        HtmlTreeSink::new(Html::new_document()),
        ParseOpts::default(),
    );
    let html = parser.one(document);
    let text = html.html();

    let text_chars = text.trim().to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&text_chars);
    let tokens = lex.get_tokens(stop_words);
    tokens
}

/// Parses an XML document, extracts all character data (text content),
/// tokenizes it, and removes stop words.
///
/// # Arguments
/// * `filepath` - The path to the XML file.
/// * `err_handler` - A sender for logging messages.
/// * `stop_words` - A slice of stop words to filter out.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of processed tokens on success, or an
/// `anyhow::Error` on failure.
pub fn parse_xml_document(
    filepath: &Path,
    err_handler: Arc<RwLock<mpsc::Sender<Message>>>,
    stop_words: &[String],
) -> Vec<String> {
    {
        let _ = err_handler
            .read()
            .unwrap()
            .send(Message::Info(format!("Indexing document: {filepath:?}")));
    }

    let file = match File::open(filepath) {
        Ok(f) => f,
        Err(err) => {
            let _ = err_handler
                .read()
                .unwrap()
                .send(Message::Error(format!("{err}")));
            return Vec::new();
        }
    };
    let file = BufReader::new(file);

    let parser = EventReader::new(file);
    let mut tokens = Vec::new();

    for e in parser {
        match e {
            Ok(XmlEvent::Characters(text)) => {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut lex = Lexer::new(&text_chars);
                tokens.append(&mut lex.get_tokens(stop_words));
            }
            Err(err) => {
                let _ = err_handler
                    .read()
                    .unwrap()
                    .send(Message::Error(format!("{err}")));
                continue;
            }
            _ => {}
        }
    }
    tokens
}

/// Parses a PDF document, extracts text from all pages, tokenizes it,
/// and removes stop words.
///
/// # Arguments
/// * `filepath` - The path to the PDF file.
/// * `err_handler` - A sender for logging messages.
/// * `stop_words` - A slice of stop words to filter out.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of processed tokens on success, or an
///  `anyhow::Error` on failure.
pub fn parse_pdf_document(
    filepath: &Path,
    err_handler: Arc<RwLock<mpsc::Sender<Message>>>,
    stop_words: &[String],
) -> Vec<String> {
    {
        let _ = err_handler
            .read()
            .unwrap()
            .send(Message::Info(format!("Indexing document: {filepath:?}")));
    }

    let mut tokens = Vec::new();
    let doc = match lopdf::Document::load(filepath) {
        Ok(doc) => doc,
        Err(err) => {
            let _ = err_handler
                .read()
                .unwrap()
                .send(Message::Error(format!("{err}")));
            return Vec::new();
        }
    };

    for (page_num, _) in doc.get_pages() {
        if let Ok(text) = doc.extract_text(&[page_num]) {
            let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
            let mut lexer = Lexer::new(&text_chars);
            tokens.append(&mut lexer.get_tokens(stop_words));
        }
    }

    tokens
}

/// Parses a plain text document, reads its content, tokenizes it,
/// and removes stop words.
///
/// # Arguments
/// * `filepath` - The path to the text file.
/// * `err_handler` - A sender for logging messages.
/// * `stop_words` - A slice of stop words to filter out.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of processed tokens on success, or an
/// `anyhow::Error` on failure.
pub fn parse_txt_document(
    filepath: &Path,
    err_handler: Arc<RwLock<mpsc::Sender<Message>>>,
    stop_words: &[String],
) -> Vec<String> {
    {
        let _ = err_handler
            .read()
            .unwrap()
            .send(Message::Info(format!("Indexing document: {filepath:?}")));
    }
    let content = match fs::read_to_string(filepath) {
        Ok(val) => val,
        Err(err) => {
            let _ = err_handler
                .read()
                .unwrap()
                .send(Message::Error(format!("{err}")));
            return Vec::new();
        }
    };

    let content = content.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&content);
    let tokens = lex.get_tokens(stop_words);
    tokens
}
