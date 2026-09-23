use super::*;

pub(crate) fn read_request(stream: &mut TcpStream, limits: ServerLimits) -> Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let first_line = read_http_line(&mut reader, limits.header_line_bytes)?;
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
        let line = read_http_line(&mut reader, limits.header_line_bytes)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if headers.len() >= limits.header_count {
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
    if length > limits.body_bytes {
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

pub(crate) fn read_http_line(reader: &mut impl BufRead, limit: usize) -> Result<String> {
    let mut bytes = Vec::new();
    let take_limit = u64::try_from(limit)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let read = reader.take(take_limit).read_until(b'\n', &mut bytes)?;
    if read > limit {
        bail!("request header line exceeds standalone server limit");
    }
    String::from_utf8(bytes).context("request headers must be UTF-8")
}

impl Request {
    pub(crate) fn require_json_content_type(&self) -> Result<()> {
        let is_json = self
            .headers
            .get("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"));
        if !is_json {
            bail!("Content-Type must be application/json");
        }
        Ok(())
    }

    pub(crate) fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        self.require_json_content_type()?;
        serde_json::from_slice(&self.body).context("request body is not valid JSON")
    }

    pub(crate) fn json_or_default<T: for<'de> Deserialize<'de> + Default>(&self) -> Result<T> {
        self.require_json_content_type()?;
        if self.body.is_empty() {
            Ok(T::default())
        } else {
            self.json()
        }
    }
}

pub(crate) fn authorize_operator(expected: Option<&str>, request: &Request) -> Result<()> {
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

pub(crate) fn write_response(stream: &mut TcpStream, response: Response) -> Result<()> {
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

pub(crate) fn parse_query(raw: &str) -> Result<BTreeMap<String, String>> {
    let mut query = BTreeMap::new();
    for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(percent_decode(name)?, percent_decode(value)?);
    }
    Ok(query)
}

pub(crate) fn percent_decode(raw: &str) -> Result<String> {
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
                bytes.push(u8::from_str_radix(text, HEX_RADIX)?);
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).context("URL is not UTF-8")
}

pub(crate) fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

pub(crate) fn registration_page() -> String {
    format!(
        r#"<!doctype html><html><head><title>Join creator directory</title><style>{}</style></head><body><main><h1>Join the local creator directory</h1><label>Name<input id="name"></label><label>Email<input id="email" type="email"></label><label>Languages, comma separated<input id="languages"></label><label>Markets, comma separated<input id="markets"></label><label>Niches, comma separated<input id="niches"></label><label>Platform<input id="platform"></label><label>Handle<input id="handle"></label><label>Profile URL<input id="profile"></label><button onclick="register()">Create creator profile</button><pre id="result"></pre><script>
const values=id=>document.getElementById(id).value.split(',').map(value=>value.trim()).filter(Boolean);
async function register(){{const platform=document.getElementById('platform').value.trim();const handle=document.getElementById('handle').value.trim();const identities=platform&&handle?[{{platform,external_creator_id:handle,profile_url:document.getElementById('profile').value||null,metadata:{{}}}}]:[];const payload={{display_name:document.getElementById('name').value,email:document.getElementById('email').value,languages:values('languages'),markets:values('markets'),niches:values('niches'),metadata:{{}},identities}};const response=await fetch('/api/register',{{method:'POST',headers:{{'content-type':'application/json'}},body:JSON.stringify(payload)}});const data=await response.json();document.getElementById('result').textContent=response.ok?`Portal token (save it now): ${{data.portal.token}}`:(data.error||'Registration failed');}}
</script></main></body></html>"#,
        portal_css()
    )
}

pub(crate) fn operator_home() -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>UGC operations</title><style>{}</style></head><body><main><h1>Import an existing UGC ledger</h1>
<p>This operator workspace uses the same atomic ledger operation as the CLI. Start it with <code>ugc-cli --db &lt;ledger.db&gt; standalone serve</code>, then choose the exact JSON record array written by <code>ugc-cli --db &lt;source.db&gt; standalone export &lt;export.json&gt;</code>. If the server printed that an operator token is required, enter it only in the password field.</p>
<section><h2>Import workspace</h2><label>Ledger export<input id="ledger-file" type="file" accept="application/json,.json"></label><label>Operator token (only when the server requires one)<input id="operator-token" type="password" autocomplete="off"></label><button onclick="importLedger()">Import existing records</button><pre id="import-result" role="status"></pre></section>
<section><h2>File and persistence contract</h2><p>The file is a JSON array of complete ledger record envelopes. Before writing, UGC validates every record kind, typed payload, id, status, RFC 3339 timestamp, external identity, parent and secondary relationship. Assets remain separate content-addressed files and are not copied by this record import.</p><p>Accepted records and their audit entries are committed together in one SQLite transaction. The result lists <code>imported</code>, <code>unchanged</code>, <code>conflicting</code>, and <code>rejected</code> records. An exact retained record is unchanged. A different record with the same id or external identity is conflicting and never overwrites the retained record.</p></section>
<section><h2>Safe refusals and CLI</h2><p>An empty export, malformed or unsupported record, duplicate incoming id or external identity, missing or wrong-kind relationship, noncanonical payload, or retained-record conflict refuses the whole import. No partial records or audit entries are written. Import does not contact creators, publish assets, settle payments, or enqueue provider work.</p><p>For a reusable non-graphical import, run <code>ugc-cli --db &lt;ledger.db&gt; standalone import &lt;export.json&gt;</code>. During first use, run <code>ugc-cli --db &lt;ledger.db&gt; onboarding --reset --import &lt;export.json&gt;</code>. Both commands call the same store operation as this screen.</p></section>
<p>JSON endpoints:</p><ul><li><a href="/api/dashboard">Dashboard</a></li><li><a href="/api/creators">Creators</a></li><li><a href="/api/campaigns">Campaigns</a></li><li><a href="/api/conversations">Conversations</a></li></ul><script>{}</script></main></body></html>"#,
        portal_css(),
        operator_import_script(),
    )
}

pub(crate) fn operator_import_script() -> &'static str {
    r#"async function importLedger(){const output=document.getElementById('import-result');const file=document.getElementById('ledger-file').files.item(0);if(!file){output.textContent='Choose a ledger export first.';return;}let records;try{records=JSON.parse(await file.text());}catch(error){output.textContent=`Invalid JSON: ${error.message}`;return;}const token=document.getElementById('operator-token').value.trim();const headers={'content-type':'application/json'};if(token)headers.authorization=`Bearer ${token}`;const response=await fetch('/api/import',{method:'POST',headers,body:JSON.stringify(records)});const result=await response.json();output.textContent=JSON.stringify(result,null,2);}"#
}

pub(crate) fn portal_css() -> &'static str {
    "body{font-family:system-ui,sans-serif;background:#f6f7f9;color:#17202a;margin:0}main{max-width:880px;margin:48px auto;padding:32px;background:white;border-radius:16px;box-shadow:0 8px 30px #0001}section{border:1px solid #dde3ea;border-radius:10px;padding:16px;margin:12px 0}code{background:#eef1f4;padding:2px 5px;border-radius:4px}a{color:#0759c7}"
}

pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Deserialize)]
pub(crate) struct NewConversation {
    pub(crate) creator_id: String,
    pub(crate) campaign_id: Option<String>,
    pub(crate) brief_id: Option<String>,
    pub(crate) offered_compensation_minor: Option<i64>,
    pub(crate) currency: Option<String>,
    #[serde(default)]
    pub(crate) shipping_required: bool,
    pub(crate) initial_message: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct NewMessage {
    pub(crate) body: String,
    pub(crate) channel: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct EmptyRequest {}

#[derive(Deserialize)]
pub(crate) struct ReviewSubmission {
    pub(crate) status: String,
    pub(crate) feedback: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PortalReply {
    pub(crate) conversation_id: String,
    pub(crate) body: String,
}

#[derive(Deserialize)]
pub(crate) struct PortalAccept {
    pub(crate) conversation_id: String,
}

#[derive(Deserialize)]
pub(crate) struct PortalShipping {
    pub(crate) assignment_id: String,
    pub(crate) address: ShippingAddress,
}
