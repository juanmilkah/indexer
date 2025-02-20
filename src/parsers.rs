use poppler::PopplerDocument;
use scraper::{Html, Selector};
use xml::reader::XmlEvent;
use xml::EventReader;

use crate::lexer::Lexer;
use crate::models::*;

use std::fs::{self, File};
use std::io::{self, BufReader};

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

pub fn index_html_document(model: &mut Model, filepath: &str) -> io::Result<()> {
    println!("Indexing document: {filepath}");
    let file = fs::read_to_string(filepath)?;
    let document = Html::parse_document(&file);
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
    let tokens = remove_stop_words(&tokens);
    model.add_document(filepath, &tokens);
    Ok(())
}

pub fn index_xml_document(model: &mut Model, filepath: &str) -> io::Result<()> {
    println!("Indexing document: {filepath}");

    let file = File::open(filepath)?;
    let file = BufReader::new(file);

    let parser = EventReader::new(file);

    for e in parser {
        match e {
            Ok(XmlEvent::Characters(text)) => {
                let text_chars = text.to_lowercase().chars().collect::<Vec<char>>();
                let mut lex = Lexer::new(&text_chars);

                let mut tokens = Vec::new();

                while let Some(token) = lex.by_ref().next() {
                    tokens.push(token);
                }

                let tokens = remove_stop_words(&tokens);
                model.add_document(filepath, &tokens);
            }
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn index_pdf_document(model: &mut Model, filepath: &str) -> io::Result<()> {
    println!("Indexing document: {filepath}");
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
    let mut whole_doc = String::new();

    for i in 1..end {
        if let Some(page) = document.get_page(i) {
            if let Some(text) = page.get_text() {
                whole_doc.push_str(text);
            }
        }
    }
    let text_chars = whole_doc.to_lowercase().chars().collect::<Vec<char>>();
    let mut tokens = Vec::new();
    {
        let mut lex = Lexer::new(&text_chars);

        while let Some(token) = lex.by_ref().next() {
            tokens.push(token);
        }

        tokens = remove_stop_words(&tokens);
    }
    model.add_document(filepath, &tokens);

    Ok(())
}

pub fn index_text_document(model: &mut Model, filepath: &str) -> io::Result<()> {
    println!("Indexing {filepath}...");
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
    while let Some(token) = lex.by_ref().next() {
        tokens.push(token);
    }

    let tokens = remove_stop_words(&tokens);
    model.add_document(filepath, &tokens);

    Ok(())
}
