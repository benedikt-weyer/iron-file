use std::{
    collections::HashSet,
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

const APP_NAME: &str = "iron-file";
const PROFILES_DIRECTORY: &str = "profiles";
const STATE_FILE: &str = "config.toml";
const DEFAULT_PROFILE_TOML: &str = include_str!("../../../../config/default.toml");

mod defaults;
mod store;
#[cfg(test)]
mod tests;
mod types;

pub use defaults::*;
pub use store::*;
pub use types::*;
