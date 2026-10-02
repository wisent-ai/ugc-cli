//! The operator screen's half of `campaign edit`, `campaign status`, `brief
//! archive` and `creator remove`: the same UgcService calls the CLI makes,
//! behind the operator token like every other operator route.

use super::*;

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

/// The controls for the routes above, sharing the import section's operator token field.
pub(crate) fn manage_section() -> &'static str {
    r#"<section><h2>Campaigns, briefs and creators</h2>
<label>Campaign id<input id="campaign-id"></label><label>New name<input id="campaign-name"></label><label>New objective<input id="campaign-objective"></label><label>New end date<input id="campaign-due"></label><label>New budget (minor units)<input id="campaign-budget" type="number"></label>
<button onclick="editCampaign()">Save campaign changes</button><button onclick="cancelCampaign()">Cancel campaign</button>
<label>Brief id<input id="brief-id"></label><button onclick="archiveBrief()">Archive brief</button>
<label>Creator id<input id="creator-id"></label><button onclick="removeCreator()">Remove creator</button>
<pre id="manage-result" role="status"></pre></section>
<script>
async function operatorCall(method,path,body){const token=document.getElementById('operator-token').value.trim();const headers={'content-type':'application/json'};if(token)headers.authorization=`Bearer ${token}`;const response=await fetch(path,{method,headers,body:body===undefined?undefined:JSON.stringify(body)});const text=await response.text();document.getElementById('manage-result').textContent=`HTTP ${response.status}\n${text}`;}
const field=id=>document.getElementById(id).value.trim();
function editCampaign(){const changes={};if(field('campaign-name'))changes.name=field('campaign-name');if(field('campaign-objective'))changes.objective=field('campaign-objective');if(field('campaign-due'))changes.due_date=field('campaign-due');if(field('campaign-budget'))changes.budget_minor=Number(field('campaign-budget'));operatorCall('PATCH',`/api/campaigns/${encodeURIComponent(field('campaign-id'))}`,changes);}
function cancelCampaign(){operatorCall('POST',`/api/campaigns/${encodeURIComponent(field('campaign-id'))}/status`,{status:'cancelled'});}
function archiveBrief(){operatorCall('POST',`/api/briefs/${encodeURIComponent(field('brief-id'))}/archive`,{});}
function removeCreator(){operatorCall('DELETE',`/api/creators/${encodeURIComponent(field('creator-id'))}`);}
</script>"#
}
