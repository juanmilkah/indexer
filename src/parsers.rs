use poppler::PopplerDocument;
use scraper::{Html, Selector};
use xml::reader::XmlEvent;
use xml::EventReader;

use crate::lexer::Lexer;

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::Path;

pub fn remove_stop_words(tokens: &[String]) -> Vec<String> {
    let words = stop_words::get(stop_words::LANGUAGE::English);
    let mut cleaned = Vec::new();

    for token in tokens {
        if words.contains(token) {
            continue;
        }
        cleaned.push(token.to_string());
    }

    cleaned
}

pub fn parse_html_document(filepath: &Path) -> io::Result<Vec<String>> {
    println!("Indexing document: {:?}", filepath);
    let content = fs::read_to_string(filepath)?;
    let document = Html::parse_document(&content);
    let selector = Selector::parse("body").unwrap();

    let body = document.select(&selector).next().unwrap();

    let mut text = String::new();
    for node in body.text() {
        text.push_str(node);
        text.push(' ');
    }
    let text_chars = text.trim().to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&text_chars);
    let mut tokens = Vec::new();

    while let Some(token) = lex.by_ref().next() {
        tokens.push(token);
    }
    Ok(remove_stop_words(&tokens))
}

pub fn parse_xml_document(filepath: &Path) -> io::Result<Vec<String>> {
    println!("Indexing document: {:?}", filepath);

    let file = File::open(filepath)?;
    let file = BufReader::new(file);

    let parser = EventReader::new(file);
    let mut tokens = Vec::new();

    for e in parser {
        match e {
            Ok(XmlEvent::Characters(text)) => {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut lex = Lexer::new(&text_chars);

                while let Some(token) = lex.by_ref().next() {
                    tokens.push(token);
                }
            }
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
            _ => {}
        }
    }

    Ok(remove_stop_words(&tokens))
}

pub fn parse_pdf_document(filepath: &Path) -> io::Result<Vec<String>> {
    println!("Indexing document: {:?}", filepath);
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

    let end = document.get_n_pages();
    let mut tokens = Vec::new();

    for i in 1..end {
        if let Some(page) = document.get_page(i) {
            if let Some(text) = page.get_text() {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut lex = Lexer::new(&text_chars);
                while let Some(token) = lex.by_ref().next() {
                    tokens.push(token);
                }
            }
        }
    }

    Ok(remove_stop_words(&tokens))
}

pub fn parse_txt_document(filepath: &Path) -> io::Result<Vec<String>> {
    println!("Indexing document: {:?}", filepath);
    let content = match fs::read_to_string(filepath) {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Failed to read file {:?}: {err}", filepath);
            return Err(err);
        }
    };

    let content = content.to_lowercase().chars().collect::<Vec<char>>();
    let mut lex = Lexer::new(&content);
    let mut tokens = Vec::new();
    while let Some(token) = lex.by_ref().next() {
        tokens.push(token);
    }

    Ok(remove_stop_words(&tokens))
}
