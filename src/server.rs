use home::home_dir;
use tiny_http::{Method, Response, Server};

use std::fs::File;
use std::path::PathBuf;

use crate::search_term;

pub fn run_server(index_file: &str) {
    let port = "localhost:8080";
    let server = match Server::http(port) {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Failed to bind server to port {port}: {err}");
            return;
        }
    };
    println!("Server listening on port {port}");

    for mut request in server.incoming_requests() {
        println!(
            "{method} {url}",
            method = request.method(),
            url = request.url()
        );

        match &request.method() {
            Method::Get => match request.url() {
                "/" => {
                    // respond with the index.html file
                    let html_file = home_dir()
                        .unwrap_or(PathBuf::from("."))
                        .join(".indexer")
                        .join("index.html")
                        .to_string_lossy()
                        .to_string();
                    let response = Response::from_file(File::open(&html_file).unwrap());
                    let _ = request.respond(response);
                }
                _ => {
                    let response = Response::from_string(format!(
                        "Route not Allowed: {url}",
                        url = request.url()
                    ));
                    let _ = request.respond(response.with_status_code(404));
                }
            },
            Method::Post => match request.url() {
                "/query" => {
                    let mut body = String::new();
                    let _ = &request.as_reader().read_to_string(&mut body);

                    match search_term(&body, index_file) {
                        Ok(vals) => {
                            if !vals.is_empty() {
                                let vals = vals.join("\n").to_string();
                                let response = Response::from_string(vals);
                                let _ = request.respond(response);
                            } else {
                                let _ = request.respond(Response::from_string("Zero matches!"));
                            }
                        }
                        Err(err) => {
                            let response =
                                Response::from_string(format!("Failed to search for query: {err}"));
                            let _ = request.respond(response.with_status_code(500));
                        }
                    };
                }
                _ => {
                    let response = Response::from_string(format!(
                        "Route not Allowed: {url}",
                        url = request.url()
                    ));
                    let _ = request.respond(response.with_status_code(403));
                }
            },
            _ => {
                let response = Response::from_string(format!(
                    "Method Not Allowed: {method}",
                    method = request.method()
                ));
                let _ = request.respond(response.with_status_code(403));
            }
        }
    }
}
