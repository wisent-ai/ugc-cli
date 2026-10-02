use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{
    db::Store,
    model::{
        Asset, Assignment, Brief, Campaign, Connection, Creator, CreatorIdentity, Message, Payment,
        ProviderCapabilities, Publication, Shipment, ShippingAddress, Submission, UsageRights,
    },
};

mod lifecycle;
mod ugc_service_group;
mod ugc_service_a_check_rights_group;

pub use ugc_service_group::*;
pub use ugc_service_a_check_rights_group::*;
