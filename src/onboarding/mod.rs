//! First-use walkthrough. The journey Echo publishes for this product is the
//! one shipped in `onboarding_first_use.json` at the repository root, compiled
//! into the binary; this command walks that definition rather than a second
//! copy of the same words, so the screens an operator reads here are the
//! screens the control plane holds.
//!
//! The ledger is the fleet database `ugc-cli`; this operator's progress
//! through the walk is recorded in the local state directory beside the
//! assets, keyed to that ledger and the acting operator. `--reset` discards
//! the recorded attempt and replays the journey from its entry screen in the
//! same invocation.
//!
//! Nothing here contacts a creator and nothing here moves money. With
//! `--import`, the walk calls the same validated, transactional ledger import
//! as `standalone import`; otherwise it only reads local evidence.
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::{Record, Store},
    model::Campaign,
};

mod product_id;
mod report;
mod load_or_start_state;

pub use product_id::*;
pub use report::*;
pub use load_or_start_state::*;
