//! Operator routes and controls for campaign, brief and creator creation and
//! lifecycle. Each mutation calls the same UgcService method as the CLI.

use super::*;

#[derive(Deserialize)]
struct NewCampaign {
    name: String,
    brand: String,
    product: String,
    #[serde(default)]
    objective: String,
    #[serde(default)]
    markets: Vec<String>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    channels: Vec<String>,
    budget_minor: Option<i64>,
    currency: String,
    deadline: Option<String>,
}

#[derive(Deserialize)]
struct NewBrief {
    campaign_id: String,
    service_type: String,
    creative_angle: String,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    forbidden_claims: Vec<String>,
    #[serde(default)]
    required_shots: Vec<String>,
    #[serde(default)]
    talking_points: Vec<String>,
    cta: Option<String>,
    duration_min_ms: Option<i64>,
    duration_max_ms: Option<i64>,
    #[serde(default)]
    aspect_ratios: Vec<String>,
    #[serde(default)]
    raw_footage_required: bool,
    revision_limit: Option<i64>,
    #[serde(default = "empty_object")]
    rights_requirements: Value,
}

#[derive(Deserialize)]
struct NewCreator {
    display_name: String,
    email: Option<String>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    markets: Vec<String>,
    #[serde(default)]
    niches: Vec<String>,
    #[serde(default = "empty_object")]
    metadata: Value,
}

fn empty_object() -> Value {
    json!({})
}

/// What `PATCH /api/campaigns/{id}` may change; `due_date` is the campaign's end date.
#[derive(Deserialize)]
pub(crate) struct CampaignChanges {
    pub(crate) name: Option<String>,
    pub(crate) objective: Option<String>,
    pub(crate) due_date: Option<String>,
    pub(crate) budget_minor: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct StatusChange {
    pub(crate) status: String,
}

/// The answer for a lifecycle route, or `None` when the path is not one.
pub(crate) fn manage_api(core: &UgcService<'_>, request: &Request) -> Result<Option<Response>> {
    if request.method == "POST" {
        let created = match request.path.as_str() {
            "/api/campaigns" => {
                let input: NewCampaign = request.json()?;
                Some(serde_json::to_value(core.create_campaign(
                    input.name, input.brand, input.product, input.objective,
                    input.markets, input.languages, input.channels, input.budget_minor,
                    input.currency, input.deadline,
                )?)?)
            }
            "/api/briefs" => {
                let input: NewBrief = request.json()?;
                Some(serde_json::to_value(core.add_brief(
                    input.campaign_id, input.service_type, input.creative_angle,
                    input.requirements, input.forbidden_claims, input.required_shots,
                    input.talking_points, input.cta, input.duration_min_ms,
                    input.duration_max_ms, input.aspect_ratios, input.raw_footage_required,
                    input.revision_limit, input.rights_requirements,
                )?)?)
            }
            "/api/creators" => {
                let input: NewCreator = request.json()?;
                Some(serde_json::to_value(core.add_creator(
                    input.display_name, input.email, input.languages, input.markets,
                    input.niches, input.metadata,
                )?)?)
            }
            _ => None,
        };
        if let Some(value) = created {
            return Ok(Some(Response::json("HTTP/1.1 201 Created", value)));
        }
    }
    let ok = |value: Value| Ok(Some(Response::json("HTTP/1.1 200 OK", value)));
    if let Some(rest) = request.path.strip_prefix("/api/campaigns/") {
        if let Some(id) = rest.strip_suffix("/status") {
            if request.method == "POST" {
                let change: StatusChange = request.json()?;
                return ok(serde_json::to_value(core.campaign_status(id, &change.status)?)?);
            }
        } else if request.method == "PATCH" {
            let changes: CampaignChanges = request.json()?;
            return ok(serde_json::to_value(core.edit_campaign(
                rest,
                changes.name,
                changes.objective,
                changes.due_date,
                changes.budget_minor,
            )?)?);
        }
    }
    if let Some(id) = request.path.strip_prefix("/api/briefs/").and_then(|path| path.strip_suffix("/archive")) {
        if request.method == "POST" {
            return ok(serde_json::to_value(core.archive_brief(id)?)?);
        }
    }
    if let Some(id) = request.path.strip_prefix("/api/creators/") {
        if request.method == "DELETE" {
            return ok(serde_json::to_value(core.remove_creator(id)?)?);
        }
    }
    Ok(None)
}

/// The controls share the import section's operator token field.
pub(crate) fn manage_section() -> &'static str {
    r#"<section><h2>Create a campaign</h2>
<label>Name<input id="new-campaign-name"></label><label>Brand<input id="new-campaign-brand"></label><label>Product<input id="new-campaign-product"></label><label>Objective<input id="new-campaign-objective"></label>
<label>Markets (comma-separated)<input id="new-campaign-markets"></label><label>Languages (comma-separated)<input id="new-campaign-languages"></label><label>Channels (comma-separated)<input id="new-campaign-channels"></label>
<label>Budget (minor units)<input id="new-campaign-budget" type="number"></label><label>Currency<input id="new-campaign-currency"></label><label>Deadline (RFC 3339)<input id="new-campaign-deadline"></label>
<button onclick="createCampaign()">Create campaign</button></section>
<section><h2>Add a brief</h2>
<label>Campaign id<input id="new-brief-campaign"></label><label>Service type<input id="new-brief-service"></label><label>Creative angle<input id="new-brief-angle"></label>
<label>Requirements (comma-separated)<input id="new-brief-requirements"></label><label>Forbidden claims (comma-separated)<input id="new-brief-forbidden"></label><label>Required shots (comma-separated)<input id="new-brief-shots"></label><label>Talking points (comma-separated)<input id="new-brief-points"></label>
<label>Call to action<input id="new-brief-cta"></label><label>Minimum duration (ms)<input id="new-brief-min" type="number"></label><label>Maximum duration (ms)<input id="new-brief-max" type="number"></label>
<label>Aspect ratios (comma-separated)<input id="new-brief-aspects"></label><label>Raw footage required<input id="new-brief-raw" type="checkbox"></label><label>Revision limit<input id="new-brief-revisions" type="number"></label>
<label>Rights requirements (JSON object)<textarea id="new-brief-rights">{}</textarea></label>
<button onclick="addBrief()">Add brief</button></section>
<section><h2>Add a creator</h2>
<label>Display name<input id="new-creator-name"></label><label>Email<input id="new-creator-email" type="email"></label>
<label>Languages (comma-separated)<input id="new-creator-languages"></label><label>Markets (comma-separated)<input id="new-creator-markets"></label><label>Niches (comma-separated)<input id="new-creator-niches"></label>
<label>Metadata (JSON object)<textarea id="new-creator-metadata">{}</textarea></label>
<button onclick="addCreator()">Add creator</button></section>
<section><h2>Manage campaigns, briefs and creators</h2>
<label>Campaign id<input id="campaign-id"></label><label>New name<input id="campaign-name"></label><label>New objective<input id="campaign-objective"></label><label>New end date<input id="campaign-due"></label><label>New budget (minor units)<input id="campaign-budget" type="number"></label>
<button onclick="editCampaign()">Save campaign changes</button><button onclick="cancelCampaign()">Cancel campaign</button>
<label>Brief id<input id="brief-id"></label><button onclick="archiveBrief()">Archive brief</button>
<label>Creator id<input id="creator-id"></label><button onclick="removeCreator()">Remove creator</button>
<pre id="manage-result" role="status"></pre></section>
<script>
const field=id=>document.getElementById(id).value.trim();
const csv=id=>field(id).split(',').map(value=>value.trim()).filter(Boolean);
const optional=id=>field(id)||undefined;
function integer(id){const raw=field(id);if(!raw)return undefined;const value=Number(raw);if(!Number.isSafeInteger(value))throw Error(`${id} must be an integer`);return value;}
function object(id){const value=JSON.parse(field(id)||'{}');if(value===null||Array.isArray(value)||typeof value!=='object')throw Error(`${id} must be a JSON object`);return value;}
async function operatorCall(method,path,body){
 const result=document.getElementById('manage-result');
 try{
  const value=typeof body==='function'?body():body;
  const token=document.getElementById('operator-token').value.trim();
  const headers={'content-type':'application/json'};
  if(token)headers.authorization=`Bearer ${token}`;
  const response=await fetch(path,{method,headers,body:value===undefined?undefined:JSON.stringify(value)});
  result.textContent=`${method} ${path}: HTTP ${response.status}\n${await response.text()}`;
 }catch(error){result.textContent=`${method} ${path} failed: ${error}`;}
}
function createCampaign(){operatorCall('POST','/api/campaigns',()=>({name:field('new-campaign-name'),brand:field('new-campaign-brand'),product:field('new-campaign-product'),objective:field('new-campaign-objective'),markets:csv('new-campaign-markets'),languages:csv('new-campaign-languages'),channels:csv('new-campaign-channels'),budget_minor:integer('new-campaign-budget'),currency:field('new-campaign-currency'),deadline:optional('new-campaign-deadline')}));}
function addBrief(){operatorCall('POST','/api/briefs',()=>({campaign_id:field('new-brief-campaign'),service_type:field('new-brief-service'),creative_angle:field('new-brief-angle'),requirements:csv('new-brief-requirements'),forbidden_claims:csv('new-brief-forbidden'),required_shots:csv('new-brief-shots'),talking_points:csv('new-brief-points'),cta:optional('new-brief-cta'),duration_min_ms:integer('new-brief-min'),duration_max_ms:integer('new-brief-max'),aspect_ratios:csv('new-brief-aspects'),raw_footage_required:document.getElementById('new-brief-raw').checked,revision_limit:integer('new-brief-revisions'),rights_requirements:object('new-brief-rights')}));}
function addCreator(){operatorCall('POST','/api/creators',()=>({display_name:field('new-creator-name'),email:optional('new-creator-email'),languages:csv('new-creator-languages'),markets:csv('new-creator-markets'),niches:csv('new-creator-niches'),metadata:object('new-creator-metadata')}));}
function editCampaign(){operatorCall('PATCH',`/api/campaigns/${encodeURIComponent(field('campaign-id'))}`,()=>{const changes={};if(field('campaign-name'))changes.name=field('campaign-name');if(field('campaign-objective'))changes.objective=field('campaign-objective');if(field('campaign-due'))changes.due_date=field('campaign-due');if(field('campaign-budget'))changes.budget_minor=integer('campaign-budget');return changes;});}
function cancelCampaign(){operatorCall('POST',`/api/campaigns/${encodeURIComponent(field('campaign-id'))}/status`,{status:'cancelled'});}
function archiveBrief(){operatorCall('POST',`/api/briefs/${encodeURIComponent(field('brief-id'))}/archive`,{});}
function removeCreator(){operatorCall('DELETE',`/api/creators/${encodeURIComponent(field('creator-id'))}`);}
</script>"#
}
