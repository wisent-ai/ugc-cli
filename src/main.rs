mod db;
mod ecosystem;
mod media;
mod model;
mod provider;
mod onboarding;
mod secret;
mod server;
mod service;
mod standalone;
mod standalone_server;
mod sync;

use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    db::{Record, Store},
    media::QcPolicy,
    model::{
        Assignment, Brief, Campaign, Connection, Creator, CreatorIdentity, DiscoveryQuery, Message,
        Payment, PortalAccess, Publication, Shipment, ShippingAddress, Submission, UsageRights,
    },
    service::UgcService,
    standalone::{CreatorSeed, MetricInput, StandaloneService},
};

mod cli_group;
mod run_group;

pub use cli_group::*;
pub use run_group::*;
