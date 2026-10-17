//! `ugc-cli standalone serve` for a journey: the actual server on a free
//! loopback port, on the journey's database, with an operator token of its
//! own, and every request it answers kept in the journey's report.
//!
//! The server's request limits are the caller's:
//! `UGC_JOURNEY_MAX_HEADER_LINE_BYTES`, `UGC_JOURNEY_MAX_HEADER_COUNT` and
//! `UGC_JOURNEY_MAX_BODY_BYTES`, each refused by name when unset.

use crate::support::Journey;
use anyhow::{Context, Result, bail, ensure};
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};
use std::{
    env,
    io::{BufRead, BufReader},
    net::{Ipv4Addr, TcpListener},
    process::{Child, Stdio},
};
use uuid::Uuid;

/// The token source the server reads, set only in the server's environment.
const TOKEN_VARIABLE: &str = "UGC_JOURNEY_OPERATOR_TOKEN";

pub struct Server {
    child: Child,
    origin: String,
    pub token: String,
    http: Client,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("the operator server could not be stopped: {error}");
        }
    }
}

fn stated(variable: &str) -> Result<String> {
    env::var(variable).with_context(|| {
        format!("{variable} must state the operator server's limit for this journey")
    })
}

/// Start the server on a free loopback port and return once it says it is
/// ready.
pub fn serve(journey: &Journey) -> Result<Server> {
    let header_line = stated("UGC_JOURNEY_MAX_HEADER_LINE_BYTES")?;
    let header_count = stated("UGC_JOURNEY_MAX_HEADER_COUNT")?;
    let body = stated("UGC_JOURNEY_MAX_BODY_BYTES")?;
    // Port 0 asks the system for a free port, which the server then binds.
    let free = TcpListener::bind(format!("{}:0", Ipv4Addr::LOCALHOST))?.local_addr()?;
    let bind = free.to_string();
    let token = Uuid::new_v4().to_string();
    let mut child = journey
        .command(&[
            "standalone",
            "serve",
            "--bind",
            &bind,
            "--operator-token-source",
            &format!("env:{TOKEN_VARIABLE}"),
            "--max-header-line-bytes",
            &header_line,
            "--max-header-count",
            &header_count,
            "--max-body-bytes",
            &body,
        ])
        .env(TOKEN_VARIABLE, &token)
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().context("the server has no stderr")?;
    let mut said = Vec::new();
    for line in BufReader::new(stderr).lines() {
        let line = line?;
        let ready = line.contains("standalone UGC server ready on");
        said.push(line);
        if ready {
            return Ok(Server {
                child,
                origin: format!("http://{bind}"),
                token,
                http: Client::new(),
            });
        }
    }
    bail!("the operator server stopped before it was ready: {said:?}")
}

impl Server {
    fn call(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: &Value,
        token: &str,
    ) -> Result<(bool, Value)> {
        let response = self
            .http
            .request(method.clone(), format!("{}{path}", self.origin))
            .bearer_auth(token)
            .json(body)
            .send()?;
        let status = response.status();
        let answer: Value = serde_json::from_str(&response.text()?)?;
        journey.commands.push(json!({
            "http": format!("{method} {path}"),
            "body": body,
            "status": status.as_u16(),
            "response": answer,
        }));
        Ok((status.is_success(), answer))
    }

    /// The answer to a request carrying the operator token, which must
    /// succeed.
    pub fn accepted(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: Value,
    ) -> Result<Value> {
        let (success, answer) = self.call(journey, method.clone(), path, &body, &self.token)?;
        ensure!(success, "{method} {path} was refused: {answer}");
        Ok(answer)
    }

    /// A request carrying `token` must be refused with `cause` in its error.
    pub fn refused(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: Value,
        token: &str,
        cause: &str,
    ) -> Result<()> {
        let (success, answer) = self.call(journey, method.clone(), path, &body, token)?;
        ensure!(!success, "{method} {path} was accepted: {answer}");
        ensure!(
            answer["error"]
                .as_str()
                .is_some_and(|error| error.contains(cause)),
            "{method} {path} was refused without naming `{cause}`: {answer}"
        );
        Ok(())
    }
}
