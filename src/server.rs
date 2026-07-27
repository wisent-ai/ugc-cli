use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::{db::Store, sync};

pub fn serve(
    store: &Store,
    connection_id: &str,
    bind: &str,
    actor: &str,
    once: bool,
) -> Result<()> {
    let listener =
        TcpListener::bind(bind).with_context(|| format!("cannot bind webhook server to {bind}"))?;
    eprintln!("webhook listener ready on http://{bind}/webhooks/{connection_id}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let response =
                    handle(store, connection_id, &mut stream, actor).unwrap_or_else(|error| {
                        (
                            "HTTP/1.1 400 Bad Request",
                            json!({"error": error.to_string()}),
                        )
                    });
                write_response(&mut stream, response)?;
            }
            Err(error) => eprintln!("webhook connection failed: {error}"),
        }
        if once {
            break;
        }
    }
    Ok(())
}

fn handle(
    store: &Store,
    connection_id: &str,
    stream: &mut TcpStream,
    actor: &str,
) -> Result<(&'static str, Value)> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let mut request = first_line.split_whitespace();
    let method = request.next().context("request method is missing")?;
    let path = request.next().context("request path is missing")?;
    if method != "POST" {
        bail!("only POST is accepted");
    }
    if path != format!("/webhooks/{connection_id}") {
        bail!("unknown webhook path");
    }

    let mut content_length = None;
    let mut signature = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = Some(value.parse::<usize>()?);
            }
            if name == "x-ugc-signature" || name == "x-signature" {
                signature = Some(value.to_string());
            }
        }
    }
    let length = content_length.context("Content-Length is required")?;
    let mut body = vec![b' '; length];
    reader.read_exact(&mut body)?;
    let result = sync::ingest_webhook(store, connection_id, &body, signature.as_deref(), actor)?;
    Ok(("HTTP/1.1 200 OK", result))
}

fn write_response(stream: &mut TcpStream, response: (&str, Value)) -> Result<()> {
    let body = serde_json::to_vec(&response.1)?;
    write!(
        stream,
        "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.0,
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}
