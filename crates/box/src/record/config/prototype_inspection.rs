//! PROTOTYPE(content-inspection): `[egress.inspector]`, the outbound content inspector.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::process::{Filesystem, ProcessSpec};
use crate::error::ConfigError;

/// PROTOTYPE(content-inspection): the table name, as a refusal names it.
pub(crate) const TABLE: &str = "[egress.inspector]";

/// PROTOTYPE(content-inspection): `[egress.inspector]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionConfig {
    /// `https://...` or `unix:///abs/path.sock`: an inspector outside the box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint: Option<String>,
    /// The inspector Box launches in a contained leaf, speaking on stdio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<Vec<String>>,
    /// The https bearer token, as a Box credential locator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) secret: Option<InspectorSecret>,
    /// A PEM file of extra roots the https client trusts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ca_file: Option<PathBuf>,
    /// The end-to-end deadline of one call.
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u64,
    /// `command` only: literal environment for the leaf.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, String>,
    /// `command` only: what the leaf's own syscalls reach.
    #[serde(default, skip_serializing_if = "Filesystem::is_empty")]
    pub(crate) filesystem: Filesystem,
    /// `command` only: whether the leaf reaches the GPU (Metal on macOS).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) gpu: bool,
}

/// PROTOTYPE(content-inspection): `[egress.inspector] secret`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectorSecret {
    /// The locator, for example `env://INSPECTOR_TOKEN`.
    #[serde(rename = "ref")]
    pub(crate) reference: String,
}

/// PROTOTYPE(content-inspection): where the inspector is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Binding {
    Stdio,
    Unix(PathBuf),
    Https(String),
}

fn default_timeout_ms() -> u64 {
    2000
}

impl InspectionConfig {
    /// PROTOTYPE(content-inspection): refuse a table no run could honor.
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        let refuse = |reason: &str| ConfigError::Contract {
            reason: format!("{TABLE}: {reason}"),
        };
        let binding = match (&self.endpoint, &self.command) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(refuse("set exactly one of `endpoint` and `command`"));
            }
            (None, Some(_)) => Binding::Stdio,
            (Some(endpoint), None) => parse_endpoint(endpoint).map_err(|reason| refuse(&reason))?,
        };
        match binding {
            Binding::Stdio => {
                self.leaf_spec().expect("command is set").validate(TABLE)?;
            }
            Binding::Unix(_) | Binding::Https(_) => {
                if self.gpu || !self.env.is_empty() || !self.filesystem.is_empty() {
                    return Err(refuse(
                        "`gpu`, `env`, and `filesystem` apply only with `command`",
                    ));
                }
            }
        }
        match (&binding, &self.secret) {
            (Binding::Https(_), None) => {
                return Err(refuse("an https endpoint needs `secret.ref`"));
            }
            (Binding::Https(_), Some(secret)) => {
                credentials::Locator::parse_uri(&secret.reference)
                    .map_err(|error| refuse(&format!("`secret.ref`: {error}")))?;
            }
            (_, Some(_)) => return Err(refuse("`secret.ref` applies only to an https endpoint")),
            _ => {}
        }
        if self.ca_file.is_some() && !matches!(binding, Binding::Https(_)) {
            return Err(refuse("`ca_file` applies only to an https endpoint"));
        }
        if let Some(ca) = &self.ca_file
            && !ca.is_absolute()
        {
            return Err(refuse("`ca_file` must be an absolute path"));
        }
        if self.timeout_ms == 0 {
            return Err(refuse("`timeout_ms` must be positive"));
        }
        Ok(())
    }

    /// PROTOTYPE(content-inspection): the binding `validate` admitted.
    pub(crate) fn binding(&self) -> Binding {
        match &self.endpoint {
            None => Binding::Stdio,
            Some(endpoint) => parse_endpoint(endpoint).expect("validated at configure"),
        }
    }

    /// PROTOTYPE(content-inspection): the leaf's process spec, for `command`.
    pub(crate) fn leaf_spec(&self) -> Option<ProcessSpec> {
        let command = self.command.clone()?;
        Some(ProcessSpec {
            native_filesystem_policy: None,
            command,
            workspace: None,
            env: self.env.clone(),
            filesystem: self.filesystem.clone(),
            network: None,
        })
    }
}

fn parse_endpoint(endpoint: &str) -> Result<Binding, String> {
    if let Some(path) = endpoint.strip_prefix("unix://") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("a unix endpoint is `unix:///absolute/path.sock`".to_string());
        }
        return Ok(Binding::Unix(path));
    }
    if endpoint.starts_with("https://") && endpoint.len() > "https://".len() {
        return Ok(Binding::Https(endpoint.to_string()));
    }
    if endpoint.starts_with("http://") {
        return Err(
            "plain `http://` is refused: a subject holds decrypted payload data".to_string(),
        );
    }
    Err(format!(
        "`endpoint` must be `https://...` or `unix:///...`, not {endpoint:?}"
    ))
}
