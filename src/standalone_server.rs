use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    db::Store,
    media,
    model::{Assignment, Campaign, Conversation, Creator},
    service::UgcService,
    standalone::{CreatorSeed, StandaloneService},
};

pub fn serve(
    store: &Store,
    asset_dir: &Path,
    bind: &str,
    actor: &str,
    operator_token: Option<String>,
    allow_registration: bool,
) -> Result<()> {
    let address: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid bind address: {bind}"))?;
    if !address.ip().is_loopback() && operator_token.is_none() {
        bail!("non-loopback standalone server requires --operator-token-source");
    }
    let listener = TcpListener::bind(address)
        .with_context(|| format!("cannot bind standalone server to {bind}"))?;
    eprintln!("standalone UGC server ready on http://{bind}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let response = handle(
                    store,
                    asset_dir,
                    actor,
                    operator_token.as_deref(),
                    allow_registration,
                    &mut stream,
                )
                .unwrap_or_else(|error| {
                    Response::json(
                        "HTTP/1.1 400 Bad Request",
                        json!({"error": error.to_string()}),
                    )
                });
                if let Err(error) = write_response(&mut stream, response) {
                    eprintln!("standalone response failed: {error}");
                }
            }
            Err(error) => eprintln!("standalone connection failed: {error}"),
        }
    }
    Ok(())
}

struct Request {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: &'static str, value: Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&value)
                .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec()),
        }
    }

    fn html(value: String) -> Self {
        Self {
            status: "HTTP/1.1 200 OK",
            content_type: "text/html; charset=utf-8",
            body: value.into_bytes(),
        }
    }
}

fn handle(
    store: &Store,
    asset_dir: &Path,
    actor: &str,
    operator_token: Option<&str>,
    allow_registration: bool,
    stream: &mut TcpStream,
) -> Result<Response> {
    let request = read_request(stream)?;
    if request.path == "/health" {
        return Ok(Response::json(
            "HTTP/1.1 200 OK",
            json!({"healthy": true, "mode": "standalone"}),
        ));
    }
    if request.path == "/" {
        return Ok(Response::html(operator_home()));
    }
    if allow_registration && request.path == "/register" && request.method == "GET" {
        return Ok(Response::html(registration_page()));
    }
    if allow_registration && request.path == "/api/register" && request.method == "POST" {
        let seed: CreatorSeed = request.json()?;
        let standalone = StandaloneService { store, actor };
        return Ok(Response::json(
            "HTTP/1.1 201 Created",
            standalone.register_creator(seed)?,
        ));
    }
    if let Some(token) = request.path.strip_prefix("/portal/") {
        if request.method != "GET" {
            bail!("portal page accepts GET");
        }
        return portal_page(store, token, actor);
    }
    if let Some(rest) = request.path.strip_prefix("/api/portal/") {
        return portal_api(store, asset_dir, actor, rest, &request);
    }
    authorize_operator(operator_token, &request)?;
    operator_api(store, actor, &request)
}

fn operator_api(store: &Store, actor: &str, request: &Request) -> Result<Response> {
    let standalone = StandaloneService { store, actor };
    let core = UgcService { store, actor };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/dashboard") => Ok(Response::json("HTTP/1.1 200 OK", standalone.dashboard()?)),
        ("GET", "/api/creators") => Ok(Response::json(
            "HTTP/1.1 200 OK",
            serde_json::to_value(store.list::<Creator>(
                "creator",
                None,
                request.query.get("status").map(String::as_str),
            )?)?,
        )),
        ("GET", "/api/campaigns") => Ok(Response::json(
            "HTTP/1.1 200 OK",
            serde_json::to_value(store.list::<Campaign>(
                "campaign",
                None,
                request.query.get("status").map(String::as_str),
            )?)?,
        )),
        ("GET", "/api/conversations") => Ok(Response::json(
            "HTTP/1.1 200 OK",
            serde_json::to_value(standalone.list_conversations(
                request.query.get("campaign_id").map(String::as_str),
                request.query.get("creator_id").map(String::as_str),
                request.query.get("status").map(String::as_str),
            )?)?,
        )),
        ("POST", "/api/conversations") => {
            let input: NewConversation = request.json()?;
            Ok(Response::json(
                "HTTP/1.1 201 Created",
                standalone.create_conversation(
                    input.creator_id,
                    input.campaign_id,
                    input.brief_id,
                    input.offered_compensation_minor,
                    input.currency.unwrap_or_else(|| "USD".into()),
                    input.initial_message,
                )?,
            ))
        }
        _ => {
            if let Some(id) = request
                .path
                .strip_prefix("/api/conversations/")
                .and_then(|path| path.strip_suffix("/messages"))
            {
                if request.method == "GET" {
                    return Ok(Response::json(
                        "HTTP/1.1 200 OK",
                        serde_json::to_value(standalone.messages(id)?)?,
                    ));
                }
                if request.method == "POST" {
                    let input: NewMessage = request.json()?;
                    return Ok(Response::json(
                        "HTTP/1.1 201 Created",
                        serde_json::to_value(standalone.send_message(
                            id,
                            input.body,
                            input.channel.unwrap_or_else(|| "local_portal".into()),
                            false,
                        )?)?,
                    ));
                }
            }
            if let Some(id) = request
                .path
                .strip_prefix("/api/conversations/")
                .and_then(|path| path.strip_suffix("/accept"))
            {
                if request.method == "POST" {
                    let input: AcceptConversation = request.json_or_default()?;
                    return Ok(Response::json(
                        "HTTP/1.1 200 OK",
                        serde_json::to_value(
                            standalone.accept_conversation(id, input.shipping_required)?,
                        )?,
                    ));
                }
            }
            if let Some(id) = request
                .path
                .strip_prefix("/api/submissions/")
                .and_then(|path| path.strip_suffix("/review"))
            {
                if request.method == "POST" {
                    let input: ReviewSubmission = request.json()?;
                    return Ok(Response::json(
                        "HTTP/1.1 200 OK",
                        serde_json::to_value(core.submission_review(
                            id,
                            &input.status,
                            input.feedback,
                        )?)?,
                    ));
                }
            }
            Ok(Response::json(
                "HTTP/1.1 404 Not Found",
                json!({"error": "route not found"}),
            ))
        }
    }
}

fn portal_api(
    store: &Store,
    asset_dir: &Path,
    actor: &str,
    rest: &str,
    request: &Request,
) -> Result<Response> {
    let standalone = StandaloneService { store, actor };
    let core = UgcService { store, actor };
    let mut segments = rest.split('/');
    let token = segments.next().context("portal token is missing")?;
    let action = segments.next();
    let access = standalone.resolve_portal(token)?;
    let creator: Creator = store.get("creator", &access.creator_id)?;
    match (request.method.as_str(), action) {
        ("GET", None) => {
            let conversations = standalone.list_conversations(None, Some(&creator.id), None)?;
            let mut threads = Vec::new();
            for conversation in conversations {
                let messages = standalone.messages(&conversation.id)?;
                threads.push(json!({"conversation": conversation, "messages": messages}));
            }
            let assignments: Vec<Assignment> = store.list("assignment", None, None)?;
            let assignments: Vec<_> = assignments
                .into_iter()
                .filter(|assignment| assignment.creator_id == creator.id)
                .collect();
            Ok(Response::json(
                "HTTP/1.1 200 OK",
                json!({"creator": creator, "threads": threads, "assignments": assignments}),
            ))
        }
        ("POST", Some("reply")) => {
            let input: PortalReply = request.json()?;
            let conversation: Conversation = store.get("conversation", &input.conversation_id)?;
            if conversation.creator_id != creator.id {
                bail!("conversation does not belong to portal creator");
            }
            Ok(Response::json(
                "HTTP/1.1 201 Created",
                standalone.receive_message(
                    &input.conversation_id,
                    input.body,
                    "local_portal".into(),
                    None,
                )?,
            ))
        }
        ("POST", Some("accept")) => {
            let input: PortalAccept = request.json()?;
            let conversation: Conversation = store.get("conversation", &input.conversation_id)?;
            if conversation.creator_id != creator.id {
                bail!("conversation does not belong to portal creator");
            }
            Ok(Response::json(
                "HTTP/1.1 200 OK",
                serde_json::to_value(
                    standalone
                        .accept_conversation(&input.conversation_id, input.shipping_required)?,
                )?,
            ))
        }
        ("POST", Some("submission")) => {
            let input: PortalSubmission = request.json()?;
            let mut assignment: Assignment = store.get("assignment", &input.assignment_id)?;
            if assignment.creator_id != creator.id {
                bail!("assignment does not belong to portal creator");
            }
            if assignment.status == "accepted" {
                assignment = core.assignment_status(&assignment.id, "in_production")?;
            }
            if assignment.status == "revision_requested" {
                assignment = core.assignment_status(&assignment.id, "in_production")?;
            }
            if assignment.status != "in_production" {
                bail!("assignment is not ready for a submission");
            }
            let submission = core.add_submission(assignment.id.clone(), None)?;
            let asset = media::import_asset(
                store,
                asset_dir,
                Path::new(&input.file_path),
                Some(&submission.id),
                input.role.as_deref().unwrap_or("final"),
                None,
                actor,
            )?;
            let assignment = core.assignment_status(&assignment.id, "submitted")?;
            Ok(Response::json(
                "HTTP/1.1 201 Created",
                json!({"assignment": assignment, "submission": submission, "asset": asset}),
            ))
        }
        _ => Ok(Response::json(
            "HTTP/1.1 404 Not Found",
            json!({"error": "portal route not found"}),
        )),
    }
}

fn portal_page(store: &Store, token: &str, actor: &str) -> Result<Response> {
    let standalone = StandaloneService { store, actor };
    let access = standalone.resolve_portal(token)?;
    let creator: Creator = store.get("creator", &access.creator_id)?;
    let conversations = standalone.list_conversations(None, Some(&creator.id), None)?;
    let assignments: Vec<Assignment> = store.list("assignment", None, None)?;
    let assignments: Vec<_> = assignments
        .into_iter()
        .filter(|assignment| assignment.creator_id == creator.id)
        .collect();
    let token_json = serde_json::to_string(token)?;
    let mut html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>UGC creator portal</title><style>{}</style></head><body><main><h1>Welcome, {}</h1><p>Reply, accept assignments, and submit local media without any external platform.</p><div id=\"notice\" role=\"status\"></div>",
        portal_css(),
        escape_html(&creator.display_name)
    );
    html.push_str("<h2>Conversations</h2>");
    for conversation in conversations {
        html.push_str(&format!(
            "<section><strong>{}</strong><p>Stage: {} · Status: {}</p><textarea id=\"reply-{}\" placeholder=\"Write a reply\"></textarea><div class=\"actions\"><button onclick=\"replyTo('{}')\">Send reply</button><button class=\"secondary\" onclick=\"acceptConversation('{}')\">Accept offer</button></div></section>",
            escape_html(&conversation.id),
            escape_html(&conversation.stage),
            escape_html(&conversation.status),
            escape_html(&conversation.id),
            escape_html(&conversation.id),
            escape_html(&conversation.id),
        ));
    }
    html.push_str("<h2>Assignments</h2>");
    for assignment in assignments {
        html.push_str(&format!(
            "<section><strong>{}</strong><p>Status: {} · Compensation: {} {}</p><input id=\"file-{}\" placeholder=\"Absolute path to a local media file\"><button onclick=\"submitAsset('{}')\">Submit media</button></section>",
            escape_html(&assignment.id),
            escape_html(&assignment.status),
            assignment.compensation_minor.unwrap_or_default(),
            escape_html(&assignment.currency),
            escape_html(&assignment.id),
            escape_html(&assignment.id),
        ));
    }
    html.push_str(&format!(
        r#"<script>
const portalToken={token_json};
async function callPortal(action,payload){{
 const response=await fetch(`/api/portal/${{portalToken}}/${{action}}`,{{method:'POST',headers:{{'content-type':'application/json'}},body:JSON.stringify(payload)}});
 const data=await response.json(); const notice=document.getElementById('notice');
 notice.textContent=response.ok?'Saved successfully':(data.error||'Request failed'); notice.className=response.ok?'ok':'error';
 if(response.ok) setTimeout(()=>location.reload(),700);
}}
function replyTo(id){{const body=document.getElementById(`reply-${{id}}`).value;callPortal('reply',{{conversation_id:id,body}});}}
function acceptConversation(id){{callPortal('accept',{{conversation_id:id,shipping_required:false}});}}
function submitAsset(id){{const file_path=document.getElementById(`file-${{id}}`).value;callPortal('submission',{{assignment_id:id,file_path,role:'final'}});}}
</script></main></body></html>"#
    ));
    Ok(Response::html(html))
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let mut parts = first_line.split_whitespace();
    let method = parts
        .next()
        .context("request method is missing")?
        .to_string();
    let target = parts.next().context("request target is missing")?;
    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(raw_path)?;
    let query = parse_query(raw_query)?;
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or_default();
    let mut body = vec![b' '; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

impl Request {
    fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).context("request body is not valid JSON")
    }

    fn json_or_default<T: for<'de> Deserialize<'de> + Default>(&self) -> Result<T> {
        if self.body.is_empty() {
            Ok(T::default())
        } else {
            self.json()
        }
    }
}

fn authorize_operator(expected: Option<&str>, request: &Request) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let supplied = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_equal(expected.as_bytes(), supplied.as_bytes()) {
        bail!("operator authorization failed");
    }
    Ok(())
}

fn write_response(stream: &mut TcpStream, response: Response) -> Result<()> {
    write!(
        stream,
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn parse_query(raw: &str) -> Result<BTreeMap<String, String>> {
    let mut query = BTreeMap::new();
    for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(percent_decode(name)?, percent_decode(value)?);
    }
    Ok(query)
}

fn percent_decode(raw: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut input = raw.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        match byte {
            b'+' => bytes.push(b' '),
            b'%' => {
                let high = input.next().context("incomplete URL escape")?;
                let low = input.next().context("incomplete URL escape")?;
                let pair = [high, low];
                let text = std::str::from_utf8(&pair)?;
                bytes.push(u8::from_str_radix(text, "16".parse()?)?);
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).context("URL is not UTF-8")
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = "".len() as u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == "".len() as u8
}

fn registration_page() -> String {
    format!(
        r#"<!doctype html><html><head><title>Join creator directory</title><style>{}</style></head><body><main><h1>Join the local creator directory</h1><label>Name<input id="name"></label><label>Email<input id="email" type="email"></label><label>Languages, comma separated<input id="languages"></label><label>Markets, comma separated<input id="markets"></label><label>Niches, comma separated<input id="niches"></label><label>Platform<input id="platform"></label><label>Handle<input id="handle"></label><label>Profile URL<input id="profile"></label><button onclick="register()">Create creator profile</button><pre id="result"></pre><script>
const values=id=>document.getElementById(id).value.split(',').map(value=>value.trim()).filter(Boolean);
async function register(){{const platform=document.getElementById('platform').value.trim();const handle=document.getElementById('handle').value.trim();const identities=platform&&handle?[{{platform,external_creator_id:handle,profile_url:document.getElementById('profile').value||null,metadata:{{}}}}]:[];const payload={{display_name:document.getElementById('name').value,email:document.getElementById('email').value,languages:values('languages'),markets:values('markets'),niches:values('niches'),metadata:{{}},identities}};const response=await fetch('/api/register',{{method:'POST',headers:{{'content-type':'application/json'}},body:JSON.stringify(payload)}});const data=await response.json();document.getElementById('result').textContent=response.ok?`Portal token (save it now): ${{data.portal.token}}`:(data.error||'Registration failed');}}
</script></main></body></html>"#,
        portal_css()
    )
}

fn operator_home() -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>UGC operations</title><style>{}</style></head><body><main><h1>Standalone UGC operations</h1><p>JSON endpoints:</p><ul><li><a href=\"/api/dashboard\">Dashboard</a></li><li><a href=\"/api/creators\">Creators</a></li><li><a href=\"/api/campaigns\">Campaigns</a></li><li><a href=\"/api/conversations\">Conversations</a></li></ul><p>Mutations use the documented JSON API or the ugc-cli commands.</p></main></body></html>",
        portal_css()
    )
}

fn portal_css() -> &'static str {
    "body{font-family:system-ui,sans-serif;background:#f6f7f9;color:#17202a;margin:0}main{max-width:880px;margin:48px auto;padding:32px;background:white;border-radius:16px;box-shadow:0 8px 30px #0001}section{border:1px solid #dde3ea;border-radius:10px;padding:16px;margin:12px 0}code{background:#eef1f4;padding:2px 5px;border-radius:4px}a{color:#0759c7}"
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Deserialize)]
struct NewConversation {
    creator_id: String,
    campaign_id: Option<String>,
    brief_id: Option<String>,
    offered_compensation_minor: Option<i64>,
    currency: Option<String>,
    initial_message: Option<String>,
}

#[derive(Deserialize)]
struct NewMessage {
    body: String,
    channel: Option<String>,
}

#[derive(Deserialize, Default)]
struct AcceptConversation {
    #[serde(default)]
    shipping_required: bool,
}

#[derive(Deserialize)]
struct ReviewSubmission {
    status: String,
    feedback: Option<String>,
}

#[derive(Deserialize)]
struct PortalReply {
    conversation_id: String,
    body: String,
}

#[derive(Deserialize)]
struct PortalAccept {
    conversation_id: String,
    #[serde(default)]
    shipping_required: bool,
}

#[derive(Deserialize)]
struct PortalSubmission {
    assignment_id: String,
    file_path: String,
    role: Option<String>,
}
