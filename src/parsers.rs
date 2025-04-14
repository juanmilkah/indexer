use anyhow::Context;
use html5ever::driver::{self, ParseOpts};
use poppler::PopplerDocument;
use scraper::{Html, HtmlTreeSink};
use tendril::TendrilSink;
use xml::reader::XmlEvent;
use xml::EventReader;

use crate::lexer::Lexer;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};

pub fn parse_csv_document(
    filepath: &Path,
    err_handler: Arc<Mutex<mpsc::Sender<String>>>,
    stop_words: &[String],
) -> anyhow::Result<Vec<String>> {
    {
        let _ = err_handler
            .lock()
            .unwrap()
            .send(format!("Indexing document: {:?}", filepath));
    }
    let reader = BufReader::new(File::open(filepath).context("open filepath")?);
    let mut rdr = csv::Reader::from_reader(reader);

    let mut fields = String::new();

    for record in rdr.records() {
        // The iterator yieds Result<StringRecord, Error>
        let record = record.context("check record")?;
        for field in record.iter() {
            fields.push_str(field);
        }
    }

    let fields_chars = fields.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&fields_chars);
    let tokens = lex.get_tokens(stop_words);
    Ok(tokens)
}

pub fn parse_html_document(
    filepath: &Path,
    err_handler: Arc<Mutex<mpsc::Sender<String>>>,
    stop_words: &[String],
) -> anyhow::Result<Vec<String>> {
    {
        let _ = err_handler
            .lock()
            .unwrap()
            .send(format!("Indexing document: {:?}", filepath));
    }
    let document = fs::read_to_string(filepath)?;
    let parser = driver::parse_document(
        HtmlTreeSink::new(Html::new_document()),
        ParseOpts::default(),
    );
    let html = parser.one(document);
    let root = html.root_element().text();
    let mut text = String::new();
    for node in root {
        text.push_str(node);
        text.push(' ');
    }
    let text_chars = text.trim().to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&text_chars);
    let tokens = lex.get_tokens(stop_words);
    Ok(tokens)
}

pub fn parse_xml_document(
    filepath: &Path,
    err_handler: Arc<Mutex<mpsc::Sender<String>>>,
    stop_words: &[String],
) -> anyhow::Result<Vec<String>> {
    {
        let _ = err_handler
            .lock()
            .unwrap()
            .send(format!("Indexing document: {:?}", filepath));
    }

    let file = File::open(filepath)?;
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
                let _ = err_handler.lock().unwrap().send(format!("{err}"));
                continue;
            }
            _ => {}
        }
    }
    Ok(tokens)
}

pub fn parse_pdf_document(
    filepath: &Path,
    err_handler: Arc<Mutex<mpsc::Sender<String>>>,
    stop_words: &[String],
) -> anyhow::Result<Vec<String>> {
    {
        let _ = err_handler
            .lock()
            .unwrap()
            .send(format!("Indexing document: {:?}", filepath));
    }
    let document = match PopplerDocument::new_from_file(filepath, None) {
        Ok(doc) => doc,
        Err(err) => {
            {
                let _ = err_handler
                    .lock()
                    .unwrap()
                    .send(format!("Failed to load document: {err}"));
            }
            return Err(anyhow::Error::new(err));
        }
    };

    let end = document.get_n_pages();
    let mut tokens = Vec::new();

    // I tried processing the doc in parallel
    // Apparently the `poppler` crate does not impl `Send`
    // I guess you'll just have to suck it up for huge pdfs
    for i in 1..end {
        if let Some(page) = document.get_page(i) {
            if let Some(text) = page.get_text() {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut lex = Lexer::new(&text_chars);
                tokens.append(&mut lex.get_tokens(stop_words));
            }
        }
    }

    Ok(tokens)
}

pub fn parse_txt_document(
    filepath: &Path,
    err_handler: Arc<Mutex<mpsc::Sender<String>>>,
    stop_words: &[String],
) -> anyhow::Result<Vec<String>> {
    {
        let _ = err_handler
            .lock()
            .unwrap()
            .send(format!("Indexing document: {:?}", filepath));
    }
    let content = match fs::read_to_string(filepath) {
        Ok(val) => val,
        Err(err) => {
            {
                let _ = err_handler
                    .lock()
                    .unwrap()
                    .send(format!("Failed to read file {:?} : {err}", filepath));
            }
            return Err(anyhow::Error::new(err));
        }
    };

    let content = content.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&content);
    let tokens = lex.get_tokens(stop_words);
    Ok(tokens)
}
