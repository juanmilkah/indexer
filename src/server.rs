use tiny_http::{Header, Method, Response, Server};

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::html::HTML_DEFAULT;
use crate::{search_term, ErrorHandler};

pub fn run_server(
    index_file: &Path,
    port: u16,
    err_handler: Arc<Mutex<ErrorHandler>>,
) -> io::Result<()> {
    let port = format!("localhost:{port}");
    let server = match Server::http(&port) {
        Ok(val) => val,
        Err(err) => {
            err_handler
                .lock()
                .unwrap()
                .print(&format!("Failed to bind server to port {port}: {err}"));
            return Err(io::Error::new(io::ErrorKind::ConnectionRefused, err));
        }
    };
    println!("Server listening on port {port}");

    for mut request in server.incoming_requests() {
        err_handler.lock().unwrap().print(&format!(
            "{method} {url}",
            method = request.method(),
            url = request.url()
        ));

        match &request.method() {
            Method::Get => match request.url() {
                "/" => {
                    let header = Header::from_bytes("Content-Type", "text/html").unwrap();
                    let response = Response::from_string(HTML_DEFAULT).with_header(header);
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
                                let vals: Vec<u8> = vals
                                    .iter()
                                    .flat_map(|path| {
                                        let path = path.to_string_lossy();
                                        path.as_bytes().to_vec()
                                    })
                                    .collect();
                                let response = Response::from_data(vals);
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

    Ok(())
}
