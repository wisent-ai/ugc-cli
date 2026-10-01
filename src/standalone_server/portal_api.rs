use super::*;

pub(crate) fn portal_api(
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

pub(crate) fn portal_page(store: &Store, token: &str, actor: &str) -> Result<Response> {
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
 if(response.ok) location.reload();
}}
function replyTo(id){{const body=document.getElementById(`reply-${{id}}`).value;callPortal('reply',{{conversation_id:id,body}});}}
function acceptConversation(id){{callPortal('accept',{{conversation_id:id}});}}
function saveShipping(id){{const value=name=>document.getElementById(`${{name}}-${{id}}`).value;const optional=name=>{{const item=value(name).trim();return item||null;}};callPortal('shipping',{{assignment_id:id,address:{{recipient_name:value('ship-name'),line1:value('ship-line1'),line2:optional('ship-line2'),city:value('ship-city'),region:optional('ship-region'),postal_code:value('ship-postal'),country:value('ship-country')}}}});}}
async function submitAsset(id){{const input=document.getElementById(`file-${{id}}`);const file=input.files.item(''.length);const notice=document.getElementById('notice');if(!file){{notice.textContent='Select a media file first';notice.className='error';return;}}const response=await fetch(`/api/portal/${{portalToken}}/submission`,{{method:'POST',headers:{{'content-type':'application/octet-stream','x-assignment-id':id,'x-file-name':encodeURIComponent(file.name),'x-role':'final'}},body:file}});const data=await response.json();notice.textContent=response.ok?'Media submitted successfully':(data.error||'Upload failed');notice.className=response.ok?'ok':'error';if(response.ok)location.reload();}}
</script></main></body></html>"#
    ));
    Ok(Response::html(html))
}
