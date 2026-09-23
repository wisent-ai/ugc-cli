use super::*;

pub(crate) const HEX_RADIX: u32 = 16;

#[derive(Clone, Copy)]
pub struct ServerLimits {
    pub header_line_bytes: usize,
    pub header_count: usize,
    pub body_bytes: usize,
    pub timeout_seconds: u64,
}

pub fn serve(
    store: &Store,
    asset_dir: &Path,
    bind: &str,
    actor: &str,
    operator_token: Option<String>,
    allow_registration: bool,
    portal_days: Option<i64>,
    limits: ServerLimits,
) -> Result<()> {
    let address: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid bind address: {bind}"))?;
    if !address.ip().is_loopback() && operator_token.is_none() {
        bail!("non-loopback standalone server requires --operator-token-source");
    }
    if limits.header_line_bytes == 0
        || limits.header_count == 0
        || limits.body_bytes == 0
        || limits.timeout_seconds == 0
    {
        bail!("standalone server limits must be positive");
    }
    let listener = TcpListener::bind(address)
        .with_context(|| format!("cannot bind standalone server to {bind}"))?;
    eprintln!("standalone UGC server ready on http://{bind}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let response = (|| -> Result<Response> {
                    let timeout = Duration::from_secs(limits.timeout_seconds);
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    handle(
                        store,
                        asset_dir,
                        actor,
                        operator_token.as_deref(),
                        allow_registration,
                        portal_days,
                        limits,
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

pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) query: BTreeMap<String, String>,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) struct Response {
    pub(crate) status: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
}

impl Response {
    pub(crate) fn json(status: &'static str, value: Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&value)
                .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec()),
        }
    }

    pub(crate) fn html(value: String) -> Self {
        Self {
            status: "HTTP/1.1 200 OK",
            content_type: "text/html; charset=utf-8",
            body: value.into_bytes(),
        }
    }
}

pub(crate) fn handle(
    store: &Store,
    asset_dir: &Path,
    actor: &str,
    operator_token: Option<&str>,
    allow_registration: bool,
    portal_days: Option<i64>,
    limits: ServerLimits,
    stream: &mut TcpStream,
) -> Result<Response> {
    let request = read_request(stream, limits)?;
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
            standalone.register_creator(seed, portal_days)?,
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

pub(crate) fn operator_api(store: &Store, actor: &str, request: &Request) -> Result<Response> {
    let standalone = StandaloneService { store, actor };
    let core = UgcService { store, actor };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/dashboard") => Ok(Response::json("HTTP/1.1 200 OK", standalone.dashboard()?)),
        ("POST", "/api/import") => {
            let records: Vec<Record> = request.json()?;
            let result = store.import_records(&records, actor)?;
            let status = if result.get("applied").and_then(Value::as_bool) == Some(true) {
                "HTTP/1.1 200 OK"
            } else {
                "HTTP/1.1 409 Conflict"
            };
            Ok(Response::json(status, result))
        }
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
