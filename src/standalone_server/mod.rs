use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    db::{Record, Store},
    media,
    model::{Assignment, Campaign, Conversation, Creator, ShippingAddress},
    service::UgcService,
    standalone::{CreatorSeed, StandaloneService},
};

mod hex_radix;
mod manage;
mod portal_api;
mod read_request;

pub use hex_radix::*;
pub use manage::*;
pub use portal_api::*;
pub use read_request::*;
