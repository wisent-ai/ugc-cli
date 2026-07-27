use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    db::Store,
    media,
    model::{Assignment, Campaign, Conversation, Creator, ShippingAddress},
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
                let response = (|| -> Result<Response> {
                    let timeout = Duration::from_secs(request_timeout_seconds());
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    handle(
                        store,
                        asset_dir,
                        actor,
                        operator_token.as_deref(),
                        allow_registration,
                        &mut stream,
                    )
                })()
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
            let currency = match (input.currency, input.campaign_id.as_deref()) {
                (Some(currency), _) => currency,
                (None, Some(campaign_id)) => {
                    store.get::<Campaign>("campaign", campaign_id)?.currency
                }
                (None, None) => "USD".into(),
            };
            Ok(Response::json(
                "HTTP/1.1 201 Created",
                standalone.create_conversation(
                    input.creator_id,
                    input.campaign_id,
                    input.brief_id,
                    input.offered_compensation_minor,
                    currency,
                    input.shipping_required,
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
                    let _: EmptyRequest = request.json_or_default()?;
                    return Ok(Response::json(
                        "HTTP/1.1 200 OK",
                        serde_json::to_value(standalone.accept_conversation(id)?)?,
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
                serde_json::to_value(standalone.accept_conversation(&input.conversation_id)?)?,
            ))
        }
        ("POST", Some("shipping")) => {
            let input: PortalShipping = request.json()?;
            let assignment: Assignment = store.get("assignment", &input.assignment_id)?;
            if assignment.creator_id != creator.id {
                bail!("assignment does not belong to portal creator");
            }
            if !assignment.shipping_required {
                bail!("assignment does not require product shipping");
            }
            if !matches!(assignment.status.as_str(), "accepted" | "product_shipping") {
                bail!("shipping address cannot be changed in the current assignment state");
            }
            Ok(Response::json(
                "HTTP/1.1 200 OK",
                serde_json::to_value(core.update_shipment(
                    assignment.id,
                    "ready_to_ship".into(),
                    None,
                    None,
                    None,
                    Some(input.address),
                )?)?,
            ))
        }
        ("POST", Some("submission")) => {
            if request
                .headers
                .get("content-type")
                .is_none_or(|value| !value.eq_ignore_ascii_case("application/octet-stream"))
            {
                bail!("submission Content-Type must be application/octet-stream");
            }
            if request.body.is_empty() {
                bail!("submission file is empty");
            }
            let assignment_id = request
                .headers
                .get("x-assignment-id")
                .context("X-Assignment-Id header is required")?;
            let role = request
                .headers
                .get("x-role")
                .map(String::as_str)
                .unwrap_or("final");
            if !matches!(role, "final" | "raw" | "thumbnail") {
                bail!("submission role must be final, raw, or thumbnail");
            }
            let encoded_name = request
                .headers
                .get("x-file-name")
                .context("X-File-Name header is required")?;
            let decoded_name = percent_decode(encoded_name)?;
            let file_name = Path::new(&decoded_name)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .context("submission file name is invalid")?;
            let mut assignment: Assignment = store.get("assignment", assignment_id)?;
            if assignment.creator_id != creator.id {
                bail!("assignment does not belong to portal creator");
            }
            if assignment.status == "accepted" && assignment.shipping_required {
                bail!("product delivery must be completed before media submission");
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
            let incoming_dir = asset_dir.join(".incoming");
            fs::create_dir_all(&incoming_dir)?;
            let incoming_path = incoming_dir.join(format!("{}-{file_name}", Store::id()));
            fs::write(&incoming_path, &request.body)?;
            let imported =
                media::import_asset(store, asset_dir, &incoming_path, None, role, None, actor);
            let cleanup_error = fs::remove_file(&incoming_path).err();
            let mut asset = imported?;
            if let Some(error) = cleanup_error {
                eprintln!("standalone upload cleanup failed: {error}");
            }
            let submission = core.add_submission(assignment.id.clone(), None)?;
            asset.submission_id = Some(submission.id.clone());
            store.put(
                "asset",
                &asset.id,
                Some(&submission.id),
                None,
                "available",
                Some(&asset.sha256),
                &asset,
                &asset.created_at,
            )?;
            store.audit(
                "asset",
                &asset.id,
                "submission_attached",
                actor,
                &json!({"submission_id": submission.id}),
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
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>UGC creator portal</title><style>{}</style></head><body><main><h1>Welcome, {}</h1><p>Reply, accept assignments, and upload media directly without any external platform.</p><div id=\"notice\" role=\"status\"></div>",
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
        if assignment.shipping_required
            && matches!(assignment.status.as_str(), "accepted" | "product_shipping")
        {
            html.push_str(&format!(
                "<section><strong>Shipping address</strong><input id=\"ship-name-{}\" placeholder=\"Recipient name\"><input id=\"ship-line1-{}\" placeholder=\"Address line\"><input id=\"ship-line2-{}\" placeholder=\"Address line 2 (optional)\"><input id=\"ship-city-{}\" placeholder=\"City\"><input id=\"ship-region-{}\" placeholder=\"Region (optional)\"><input id=\"ship-postal-{}\" placeholder=\"Postal code\"><input id=\"ship-country-{}\" placeholder=\"Country code\"><button onclick=\"saveShipping('{}')\">Save shipping address</button></section>",
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
                escape_html(&assignment.id),
            ));
        }
        html.push_str(&format!(
            "<section><strong>{}</strong><p>Status: {} · Compensation: {} {}</p>",
            escape_html(&assignment.id),
            escape_html(&assignment.status),
            assignment.compensation_minor.unwrap_or_default(),
            escape_html(&assignment.currency),
        ));
        if matches!(
            assignment.status.as_str(),
            "in_production" | "revision_requested"
        ) {
            html.push_str(&format!(
                "<input id=\"file-{}\" type=\"file\" accept=\"video/*,image/*,audio/*\"><button onclick=\"submitAsset('{}')\">Submit media</button>",
                escape_html(&assignment.id),
                escape_html(&assignment.id),
            ));
        } else {
            html.push_str("<p>Media upload becomes available when production starts.</p>");
        }
        html.push_str("</section>");
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
function acceptConversation(id){{callPortal('accept',{{conversation_id:id}});}}
function saveShipping(id){{const value=name=>document.getElementById(`${{name}}-${{id}}`).value;const optional=name=>{{const item=value(name).trim();return item||null;}};callPortal('shipping',{{assignment_id:id,address:{{recipient_name:value('ship-name'),line1:value('ship-line1'),line2:optional('ship-line2'),city:value('ship-city'),region:optional('ship-region'),postal_code:value('ship-postal'),country:value('ship-country')}}}});}}
async function submitAsset(id){{const input=document.getElementById(`file-${{id}}`);const file=input.files.item(''.length);const notice=document.getElementById('notice');if(!file){{notice.textContent='Select a media file first';notice.className='error';return;}}const response=await fetch(`/api/portal/${{portalToken}}/submission`,{{method:'POST',headers:{{'content-type':'application/octet-stream','x-assignment-id':id,'x-file-name':encodeURIComponent(file.name),'x-role':'final'}},body:file}});const data=await response.json();notice.textContent=response.ok?'Media submitted successfully':(data.error||'Upload failed');notice.className=response.ok?'ok':'error';if(response.ok)location.reload();}}
</script></main></body></html>"#
    ));
    Ok(Response::html(html))
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let first_line = read_http_line(&mut reader)?;
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
        let line = read_http_line(&mut reader)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if headers.len() >= max_request_header_count() {
            bail!("too many request headers");
        }
        let (name, value) = line
            .split_once(':')
            .context("request header is malformed")?;
        let name = name.trim().to_ascii_lowercase();
        if headers
            .insert(name.clone(), value.trim().to_string())
            .is_some()
        {
            bail!("duplicate request header: {name}");
        }
    }
    if headers.contains_key("transfer-encoding") {
        bail!("Transfer-Encoding is not supported");
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or_default();
    if length > max_request_body_bytes() {
        bail!("request body exceeds standalone server limit");
    }
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

fn read_http_line(reader: &mut impl BufRead) -> Result<String> {
    let limit = max_request_header_line_bytes();
    let mut bytes = Vec::new();
    let read = reader
        .take(limit.saturating_add(usize::from(true)))
        .read_until(b'\n', &mut bytes)?;
    if read > limit {
        bail!("request header line exceeds standalone server limit");
    }
    String::from_utf8(bytes).context("request headers must be UTF-8")
}

impl Request {
    fn require_json_content_type(&self) -> Result<()> {
        let is_json = self
            .headers
            .get("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"));
        if !is_json {
            bail!("Content-Type must be application/json");
        }
        Ok(())
    }

    fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        self.require_json_content_type()?;
        serde_json::from_slice(&self.body).context("request body is not valid JSON")
    }

    fn json_or_default<T: for<'de> Deserialize<'de> + Default>(&self) -> Result<T> {
        self.require_json_content_type()?;
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
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'\r\nCache-Control: no-store\r\n\r\n",
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
    #[serde(default)]
    shipping_required: bool,
    initial_message: Option<String>,
}
fn max_request_header_line_bytes() -> usize {
    usize::from_str_radix("4000", "security".len()).expect("valid request header line limit")
}

fn max_request_header_count() -> usize {
    "100".parse().expect("valid request header count limit")
}

#[derive(Deserialize)]
struct NewMessage {
    body: String,
    channel: Option<String>,
}

#[derive(Deserialize, Default)]
struct EmptyRequest {}

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
}

#[derive(Deserialize)]
struct PortalShipping {
    assignment_id: String,
    address: ShippingAddress,
}

fn max_request_body_bytes() -> usize {
    "104857600".parse().expect("valid request body limit")
}

fn request_timeout_seconds() -> u64 {
    "30".parse().expect("valid request timeout")
}
