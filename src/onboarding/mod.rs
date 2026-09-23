//! First-use walkthrough. The journey Echo publishes for this product is the
//! one shipped in `onboarding_first_use.json` at the repository root, compiled
//! into the binary; this command walks that definition rather than a second
//! copy of the same words, so the screens an operator reads here are the
//! screens the control plane holds.
//!
//! Progress is recorded beside the ledger it describes, because the evidence
//! this journey waits for is the first campaign record in that database: a
//! scratch `--db` therefore rehearses the whole walk without touching the
//! progress of a working ledger. `--reset` discards the recorded attempt and
//! replays the journey from its entry screen in the same invocation.
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
