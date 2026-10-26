//! The tool pack file as its author writes it.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One `*.pack.toml` document.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackFile {
    /// Read by `Pack::parse` before the document deserializes.
    #[serde(rename = "format")]
    pub(crate) _format: serde::de::IgnoredAny,
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
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalSpec {
    #[serde(default)]
    pub(crate) options: BTreeMap<String, OptionSpec>,
}

/// One `[shell.<action>]` table.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
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
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
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

/// One `operands.<name>` entry.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
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

/// One `when` pattern rule on an option or an operand.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WhenRule {
    pub(crate) like: Patterns,
    #[serde(default)]
    pub(crate) adds: Vec<String>,
    #[serde(default)]
    pub(crate) removes: Vec<String>,
}

/// One pattern or several.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Patterns {
    One(String),
    Many(Vec<String>),
}

/// One flag spelling or several.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Flags {
    One(String),
    Many(Vec<String>),
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
