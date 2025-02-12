use std::io;
use std::str;
use std::sync::{Arc, Mutex};

use seroost_lib::model::*;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

fn serve_404(request: Request) -> io::Result<()> {
    request.respond(Response::from_string("404").with_status_code(StatusCode(404)))
}

fn serve_500(request: Request) -> io::Result<()> {
    request.respond(Response::from_string("500").with_status_code(StatusCode(500)))
}

fn serve_400(request: Request, message: &str) -> io::Result<()> {
    request
        .respond(Response::from_string(format!("400: {message}")).with_status_code(StatusCode(400)))
}

fn serve_bytes(request: Request, bytes: &[u8], content_type: &str) -> io::Result<()> {
    let content_type_header = Header::from_bytes("Content-Type", content_type)
        .expect("That we didn't put any garbage in the headers");
    request.respond(Response::from_data(bytes).with_header(content_type_header))
}

// TODO: the errors of serve_api_search should probably return JSON
// 'Cause that's what expected from them.
fn serve_api_search(model: Arc<Mutex<Model>>, mut request: Request) -> io::Result<()> {
    let mut buf = Vec::new();
    if let Err(err) = request.as_reader().read_to_end(&mut buf) {
        eprintln!("ERROR: could not read the body of the request: {err}");
        return serve_500(request);
    }

    let body = match str::from_utf8(&buf) {
        Ok(body) => body.chars().collect::<Vec<_>>(),
        Err(err) => {
            eprintln!("ERROR: could not interpret body as UTF-8 string: {err}");
            return serve_400(request, "Body must be a valid UTF-8 string");
        }
    };

    let model = model.lock().unwrap();
    let result = model.search_query(&body);

    let json = match serde_json::to_string(&result.iter().take(20).collect::<Vec<_>>()) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("ERROR: could not convert search results to JSON: {err}");
            return serve_500(request);
        }
    };

    let content_type_header = Header::from_bytes("Content-Type", "application/json")
        .expect("That we didn't put any garbage in the headers");
    request.respond(Response::from_string(json).with_header(content_type_header))
}

fn serve_api_stats(model: Arc<Mutex<Model>>, request: Request) -> io::Result<()> {
    use serde::Serialize;

    #[derive(Default, Serialize)]
    struct Stats {
        docs_count: usize,
        terms_count: usize,
    }

    let mut stats: Stats = Default::default();
    {
        let model = model.lock().unwrap();
        stats.docs_count = model.docs.len();
        stats.terms_count = model.df.len();
    }

    let json = match serde_json::to_string(&stats) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("ERROR: could not convert stats results to JSON: {err}");
            return serve_500(request);
        }
    };

    let content_type_header = Header::from_bytes("Content-Type", "application/json")
        .expect("That we didn't put any garbage in the headers");
    request.respond(Response::from_string(json).with_header(content_type_header))
}

fn serve_request(model: Arc<Mutex<Model>>, request: Request) -> io::Result<()> {
    println!(
        "INFO: received request! method: {:?}, url: {:?}",
        request.method(),
        request.url()
    );

    match (request.method(), request.url()) {
        (Method::Post, "/api/search") => serve_api_search(model, request),
        (Method::Get, "/api/stats") => serve_api_stats(model, request),
        (Method::Get, "/index.js") => serve_bytes(
            request,
            include_bytes!("index.js"),
            "text/javascript; charset=utf-8",
        ),
        (Method::Get, "/") | (Method::Get, "/index.html") => serve_bytes(
            request,
            include_bytes!("index.html"),
            "text/html; charset=utf-8",
        ),
        _ => serve_404(request),
    }
}

pub fn start(address: &str, model: Arc<Mutex<Model>>) -> Result<(), ()> {
    let server = Server::http(address).map_err(|err| {
        eprintln!("ERROR: could not start HTTP server at {address}: {err}");
    })?;

    println!("INFO: listening at http://{address}/");

    for request in server.incoming_requests() {
        serve_request(Arc::clone(&model), request)
            .map_err(|err| {
                eprintln!("ERROR: could not serve the response: {err}");
            })
            .ok(); // <- don't stop on errors, keep serving
    }

    eprintln!("ERROR: the server socket has shutdown");
    Err(())
}
