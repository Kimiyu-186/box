//! The pack file as an operator writes it.

// `value` and `like` are accepted now and read by the full parser.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Deserialize;

/// One `*.pack.toml` document.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackFile {
    pub(crate) pack: String,
    pub(crate) program: String,
    #[serde(default)]
    pub(crate) effects: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) shell: BTreeMap<String, ShellActionSpec>,
}

/// One `[shell.<action>]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShellActionSpec {
    pub(crate) usage: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) options: BTreeMap<String, OptionSpec>,
    #[serde(default)]
    pub(crate) operands: BTreeMap<String, OperandSpec>,
}

/// One `options.<name>` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionSpec {
    pub(crate) flag: Flags,
    #[serde(default)]
    pub(crate) value: OptionValue,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
}

/// One spelling or several.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Flags {
    One(String),
    Many(Vec<String>),
}

impl Flags {
    pub(crate) fn spellings(&self) -> Vec<String> {
        match self {
            Self::One(flag) => vec![flag.clone()],
            Self::Many(flags) => flags.clone(),
        }
    }
}

/// Whether an option takes a value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OptionValue {
    #[default]
    None,
    Required,
    Optional,
}

/// One `operands.<name>` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperandSpec {
    #[serde(default)]
    pub(crate) like: Option<String>,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
}
