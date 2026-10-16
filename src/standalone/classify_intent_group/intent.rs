use super::*;

/// What a creator's inbound message asks of the conversation, as Brama
/// judges it. Each intent moves the conversation and chooses the automatic
/// reply; the meanings below are what Brama chooses between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    OptOut,
    Declined,
    Accepted,
    Interested,
    Pricing,
    Submitted,
    Question,
    Other,
}

/// Every intent with what it means, as Brama is asked to choose.
const INTENTS: &[(Intent, &str)] = &[
    (
        Intent::OptOut,
        "The creator asks not to be contacted again.",
    ),
    (Intent::Declined, "The creator declines this collaboration."),
    (
        Intent::Accepted,
        "The creator accepts the offer as it stands.",
    ),
    (
        Intent::Interested,
        "The creator is interested and wants to continue, without accepting yet.",
    ),
    (
        Intent::Pricing,
        "The creator asks about or proposes compensation.",
    ),
    (
        Intent::Submitted,
        "The creator says the content has been delivered or uploaded.",
    ),
    (
        Intent::Question,
        "The creator asks something else about the campaign.",
    ),
    (Intent::Other, "None of the above."),
];

/// The settings the judgement reads, named as `ugc brama` reads them.
const BRAMA_URL: &str = "BRAMA_URL";
const AGENT_ID: &str = "UGC_BRAMA_AGENT_ID";
const SIGNING_SECRET_SOURCE: &str = "UGC_BRAMA_SIGNING_SECRET_SOURCE";
/// The key the intent question is asked and answered under.
const QUESTION: &str = "intent";

impl Intent {
    /// The name the intent is asked and stored under.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Intent::OptOut => "opt_out",
            Intent::Declined => "declined",
            Intent::Accepted => "accepted",
            Intent::Interested => "interested",
            Intent::Pricing => "pricing",
            Intent::Submitted => "submitted",
            Intent::Question => "question",
            Intent::Other => "other",
        }
    }

    /// The intent named `name`, if it is one of [`INTENTS`].
    fn find(name: &str) -> Option<Intent> {
        INTENTS
            .iter()
            .find(|(intent, _)| intent.name() == name)
            .map(|(intent, _)| *intent)
    }

    fn named(name: &str) -> Result<Intent> {
        match Intent::find(name) {
            Some(intent) => Ok(intent),
            None => bail!("Brama chose the intent `{name}`, which is not one of the intents asked"),
        }
    }

    /// The intent the operator states by `name`, refused with every
    /// intent's name when it is none of them.
    fn stated(name: &str) -> Result<Intent> {
        match Intent::find(name) {
            Some(intent) => Ok(intent),
            None => {
                let names: Vec<&str> = INTENTS.iter().map(|(intent, _)| intent.name()).collect();
                bail!(
                    "`{name}` is not an intent; state one of {}",
                    names.join(", ")
                )
            }
        }
    }

    /// The intent of the inbound message `body`: the one the operator
    /// `stated`, or Brama's judgement when the operator stated none.
    pub(crate) fn of(stated: Option<&str>, body: &str) -> Result<Intent> {
        match stated {
            Some(name) => Intent::stated(name),
            None => judged_intent(body),
        }
    }
}

/// The value of the setting `name`, refused by name when unset or blank.
fn setting(name: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => bail!(
            "{name} is not set: an inbound creator message's intent is judged by Brama, which ugc reaches through {BRAMA_URL}, {AGENT_ID} and {SIGNING_SECRET_SOURCE}, unless the operator states it with conversation-receive --intent"
        ),
        Err(error) => bail!("{name} cannot be read: {error}"),
    }
}

/// Brama's judgement of what the inbound message `body` asks, as one typed
/// choice among [`INTENTS`].
pub(crate) fn judged_intent(body: &str) -> Result<Intent> {
    let criteria: serde_json::Map<String, Value> = INTENTS
        .iter()
        .map(|(intent, meaning)| (intent.name().to_string(), json!(meaning)))
        .collect();
    let mut questions = serde_json::Map::new();
    questions.insert(
        QUESTION.to_string(),
        json!({
            "type": "choice",
            "instructions": "What does the creator's message in the state ask of this UGC collaboration conversation? It may be written in any language.",
            "criteria": criteria,
        }),
    );
    let answers = crate::ecosystem::brama_decide(
        &setting(BRAMA_URL)?,
        &setting(AGENT_ID)?,
        &setting(SIGNING_SECRET_SOURCE)?,
        &json!({ "message": body }),
        &Value::Object(questions),
    )?;
    let chosen = answers[QUESTION]["choice"].as_str().with_context(|| {
        format!(
            "Brama's intent answer carries no choice: {}",
            answers[QUESTION]
        )
    })?;
    Intent::named(chosen)
}
