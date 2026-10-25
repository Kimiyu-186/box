//! PROTOTYPE(content-inspection): `[inspection.content]`, the outbound content inspector.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::process::{Filesystem, ProcessSpec};
use crate::error::ConfigError;

/// PROTOTYPE(content-inspection): the `[inspection]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionConfig {
    /// The content inspector.
    pub(crate) content: ContentInspection,
}

/// PROTOTYPE(content-inspection): `[inspection.content]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContentInspection {
    /// The provider kind; only `local` exists.
    pub(crate) provider: String,
    /// Mode A: a running inspector's unix socket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) socket: Option<PathBuf>,
    /// Mode B: the inspector Box launches in a contained leaf, speaking on stdio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<Vec<String>>,
    /// The per-request deadline.
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u64,
    /// Mode B: literal environment for the leaf.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, String>,
    /// Mode B: what the leaf's own syscalls reach.
    #[serde(default, skip_serializing_if = "Filesystem::is_empty")]
    pub(crate) filesystem: Filesystem,
    /// Mode B: whether the leaf reaches the GPU (Metal on macOS).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) gpu: bool,
}

fn default_timeout_ms() -> u64 {
    2000
}

impl InspectionConfig {
    /// PROTOTYPE(content-inspection): refuse a table no run could honor.
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        let content = &self.content;
        let refuse = |reason: &str| ConfigError::Contract {
            reason: format!("[inspection.content]: {reason}"),
        };
        if content.provider != "local" {
            return Err(refuse("`provider` must be \"local\""));
        }
        match (&content.socket, &content.command) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(refuse("set exactly one of `socket` and `command`"));
            }
            (Some(socket), None) if !socket.is_absolute() => {
                return Err(refuse("`socket` must be an absolute path"));
            }
            (None, Some(_)) => {
                self.leaf_spec()
                    .expect("command is set")
                    .validate("[inspection.content]")?;
            }
            _ => {}
        }
        if content.gpu && content.command.is_none() {
            return Err(refuse("`gpu` applies only with `command`"));
        }
        if content.timeout_ms == 0 {
            return Err(refuse("`timeout_ms` must be positive"));
        }
        Ok(())
    }

    /// PROTOTYPE(content-inspection): the leaf's process spec, in mode B.
    pub(crate) fn leaf_spec(&self) -> Option<ProcessSpec> {
        let command = self.content.command.clone()?;
        Some(ProcessSpec {
            native_filesystem_policy: None,
            command,
            workspace: None,
            env: self.content.env.clone(),
            filesystem: self.content.filesystem.clone(),
            network: None,
        })
    }
}
