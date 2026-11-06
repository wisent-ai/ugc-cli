use super::*;

/// What a creator's inbound message asks of the conversation, as the model
/// service Stado's capability resolution hands ugc judges it (Brama, or the
/// endpoint declared in its place). Each intent moves the conversation and
/// chooses the automatic reply; the meanings below are what it chooses between.
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

/// The settings the intent judgement reads: Brama's signed decision when
/// `BRAMA_URL` names Brama, else the chat model `UGC_INTENT_MODEL` names,
/// asked through Stado's inference alternatives.
const BRAMA_URL: &str = "BRAMA_URL";
const AGENT_ID: &str = "UGC_BRAMA_AGENT_ID";
const SIGNING_SECRET_SOURCE: &str = "UGC_BRAMA_SIGNING_SECRET_SOURCE";
const INTENT_MODEL: &str = "UGC_INTENT_MODEL";
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
            None => {
                bail!("the model chose the intent `{name}`, which is not one of the intents asked")
            }
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
    /// `stated`, or the model's judgement when the operator stated none.
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
            "{name} is not set: an inbound creator message's intent is judged by Brama signed as {AGENT_ID} with {SIGNING_SECRET_SOURCE} when {BRAMA_URL} names it, else by the model {INTENT_MODEL} names through Stado's inference alternatives, unless the operator states it with conversation-receive --intent"
        ),
        Err(error) => bail!("{name} cannot be read: {error}"),
    }
}

/// The model's judgement of what the inbound message `body` asks, as one
/// typed choice among [`INTENTS`]. When no model service answers, the
/// refusal carries its cause and the `conversation-receive --intent` the
/// operator may state instead.
pub(crate) fn judged_intent(body: &str) -> Result<Intent> {
    let criteria: serde_json::Map<String, Value> = INTENTS
        .iter()
        .map(|(intent, meaning)| (intent.name().to_string(), json!(meaning)))
        .collect();
    let named = match std::env::var(BRAMA_URL) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => bail!("{BRAMA_URL} cannot be read: {error}"),
    };
    let stated = "an inbound creator message's intent needs a model, unless the operator states it with conversation-receive --intent";
    let service = crate::ecosystem::ModelService::resolve(
        named,
        Some((Box::new(|| setting(AGENT_ID)), Box::new(|| setting(SIGNING_SECRET_SOURCE)))),
    )
    .context(stated)?;
    // Brama's decision route names its own alias; through Stado the chat
    // model the operator names is asked.
    let model = match service {
        crate::ecosystem::ModelService::Brama { .. } => String::new(),
        crate::ecosystem::ModelService::Stado => setting(INTENT_MODEL)?,
    };
    let chosen = crate::ecosystem::model_choose(
        &service,
        &model,
        &json!({ "message": body }),
        QUESTION,
        "What does the creator's message in the state ask of this UGC collaboration conversation? It may be written in any language.",
        &criteria,
    )
    .context(stated)?;
    Intent::named(&chosen)
}
