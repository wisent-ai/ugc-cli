use super::*;

/// An inbound creator message recorded on a conversation.
#[derive(Args)]
pub(crate) struct ReceiveArgs {
    pub(crate) id: String,
    #[arg(long)]
    pub(crate) body: String,
    #[arg(
        long,
        help = "The channel the message arrived on, for example local_portal"
    )]
    pub(crate) channel: String,
    #[arg(long)]
    pub(crate) external_id: Option<String>,
    #[arg(
        long,
        help = "The intent the operator states for the message, recorded in place of Brama's judgement: opt_out, declined, accepted, interested, pricing, submitted, question or other"
    )]
    pub(crate) intent: Option<String>,
}
