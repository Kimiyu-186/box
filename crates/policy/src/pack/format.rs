//! The pack file as an operator writes it.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One `*.pack.toml` document.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackFile {
    #[serde(default)]
    pub(crate) format: Option<toml::Value>,
    #[serde(default)]
    pub(crate) version: Option<toml::Value>,
    pub(crate) pack: String,
    pub(crate) program: String,
    #[serde(default)]
    pub(crate) effects: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) global: GlobalSpec,
    #[serde(default)]
    pub(crate) shell: BTreeMap<String, ShellActionSpec>,
}

/// The `[global]` table: options that come before the command words.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalSpec {
    #[serde(default)]
    pub(crate) options: BTreeMap<String, OptionSpec>,
}

/// One `[shell.<action>]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShellActionSpec {
    pub(crate) usage: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) options: BTreeMap<String, OptionSpec>,
    #[serde(default)]
    pub(crate) operands: BTreeMap<String, OperandSpec>,
}

/// One `options.<name>` entry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionSpec {
    pub(crate) flag: Flags,
    #[serde(default)]
    pub(crate) value: OptionValue,
    #[serde(default)]
    pub(crate) repeated: bool,
    #[serde(default)]
    pub(crate) like: Option<Patterns>,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
    #[serde(default)]
    pub(crate) when: Vec<WhenRule>,
    #[serde(default)]
    pub(crate) anywhere: bool,
}

/// One `when` pattern rule on an option or an operand.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WhenRule {
    pub(crate) like: Patterns,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
}

/// One pattern or several.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Patterns {
    One(String),
    Many(Vec<String>),
}

impl Patterns {
    pub(crate) fn list(&self) -> Vec<String> {
        match self {
            Self::One(pattern) => vec![pattern.clone()],
            Self::Many(patterns) => patterns.clone(),
        }
    }
}

/// One spelling or several.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Flags {
    One(String),
    Many(Vec<String>),
}

impl Default for Flags {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
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
    Many,
}

/// One `operands.<name>` entry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperandSpec {
    #[serde(default)]
    pub(crate) like: Option<Patterns>,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
    #[serde(default)]
    pub(crate) when: Vec<WhenRule>,
    #[serde(default)]
    pub(crate) rest: bool,
}
