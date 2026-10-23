//! Policy packs: an operator-installed action vocabulary for one third-party program.
//!
//! | Module | Owns |
//! |---|---|
//! | `format` | the `*.pack.toml` document as written |
//! | `grammar` | the shell grammar that turns a command line into a pack action |

#![cfg_attr(not(feature = "shell-adapter"), allow(dead_code))]

mod format;
mod grammar;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub(crate) use grammar::{FieldKind, FieldValue, Parsed, ShellGrammar};

use crate::schema::cedar_identifier;

/// The namespace every pack's vocabulary lives under.
pub(crate) const PACK_NAMESPACE: &str = "Tool";

/// The pack file language versions this Box reads.
const KNOWN_FORMATS: &[i64] = &[1];

/// The action a command for a pack's program raises when no shell action matches it.
pub(crate) const UNMODELED_ACTION: &str = "unmodeled";
/// The pure action group every action of a pack belongs to: `<namespace>::Action::"*"`.
pub(crate) const ALL_ACTIONS: &str = "*";

/// A pack file could not be loaded.
#[derive(Debug, Clone, thiserror::Error)]
#[error("tool pack {origin}: {reason}")]
pub struct PackError {
    origin: String,
    reason: String,
}

impl PackError {
    fn new(origin: &Path, reason: impl Into<String>) -> Self {
        Self {
            origin: origin.display().to_string(),
            reason: reason.into(),
        }
    }
}

/// One loaded and validated policy pack.
#[derive(Debug, Clone)]
pub struct Pack {
    origin: PathBuf,
    name: String,
    version: u32,
    program: String,
    effects: Vec<String>,
    grammar: ShellGrammar,
}

impl Pack {
    /// Parse and validate one pack document.
    pub fn parse(text: &str, origin: &Path) -> Result<Self, PackError> {
        let file: format::PackFile =
            toml::from_str(text).map_err(|error| PackError::new(origin, error.to_string()))?;
        let refuse = |reason: String| Err(PackError::new(origin, reason));

        match &file.format {
            None => return refuse("`format` is required: write `format = 1`".to_string()),
            Some(toml::Value::Integer(format)) if KNOWN_FORMATS.contains(format) => {}
            Some(other) => {
                return refuse(format!(
                    "`format = {other}` is not a pack file format this Box reads (it reads 1)"
                ));
            }
        }
        let version = match &file.version {
            None => {
                return refuse(
                    "`version` is required: write `version = 1` for a new pack".to_string(),
                );
            }
            Some(toml::Value::Integer(version)) if *version >= 1 => match u32::try_from(*version) {
                Ok(version) => version,
                Err(_) => return refuse(format!("`version = {version}` is too large")),
            },
            Some(other) => {
                return refuse(format!("`version = {other}` must be a positive integer"));
            }
        };
        if !is_identifier(&file.pack) {
            return refuse(format!(
                "`pack = {:?}` must be a plain identifier (letters, digits, '_')",
                file.pack
            ));
        }
        if file.program.is_empty()
            || file.program.contains('/')
            || file.program.chars().any(char::is_whitespace)
        {
            return refuse(format!(
                "`program = {:?}` must be one program name, not a path",
                file.program
            ));
        }
        for effect in file.effects.keys() {
            if !is_identifier(effect) {
                return refuse(format!(
                    "effect {effect:?} must be a plain identifier (letters, digits, '_')"
                ));
            }
        }
        for action in file.shell.keys() {
            if !is_identifier(action) {
                return refuse(format!(
                    "`[shell.{action}]` must be named by a plain identifier (letters, digits, '_')"
                ));
            }
            if action == UNMODELED_ACTION {
                return refuse(format!(
                    "`[shell.{UNMODELED_ACTION}]` is reserved for a command no action matches"
                ));
            }
        }
        let grammar = match ShellGrammar::compile(&file) {
            Ok(grammar) => grammar,
            Err(reason) => return refuse(reason),
        };

        Ok(Self {
            origin: origin.to_path_buf(),
            name: file.pack,
            version,
            program: file.program,
            effects: file.effects.into_keys().collect(),
            grammar,
        })
    }

    /// Where this pack was read from.
    #[must_use]
    pub fn origin(&self) -> &Path {
        &self.origin
    }

    /// The pack's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The major version of the pack's content.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The program the pack's shell grammar covers.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The Cedar namespace of this pack's vocabulary: `Tool::<name>`, or `Tool::<name>::v<N>` from 2.
    #[must_use]
    pub fn namespace(&self) -> String {
        if self.version > 1 {
            format!("{PACK_NAMESPACE}::{}::v{}", self.name, self.version)
        } else {
            format!("{PACK_NAMESPACE}::{}", self.name)
        }
    }

    pub(crate) fn grammar(&self) -> &ShellGrammar {
        &self.grammar
    }

    /// The parse of one command line as JSON, for checking a pack by hand.
    #[doc(hidden)]
    #[must_use]
    pub fn parse_command(&self, args: &[String]) -> serde_json::Value {
        match self.grammar.parse(args, Path::new("")) {
            Parsed::Unmodeled => serde_json::json!({ "action": UNMODELED_ACTION }),
            Parsed::Action {
                action,
                fields,
                effects,
            } => serde_json::json!({
                "action": action,
                "fields": fields.iter().map(|(name, value)| (name.clone(), field_json(value))).collect::<serde_json::Map<_, _>>(),
                "effects": effects,
            }),
        }
    }

    /// The generated Cedar schema fragment for this pack's vocabulary.
    #[must_use]
    pub fn schema(&self) -> String {
        let mut schema = String::new();
        let namespace = self.namespace();
        let _ = writeln!(schema, "// Generated from {}", self.origin.display());
        let _ = writeln!(schema, "namespace {namespace} {{");
        if self.effects.is_empty() {
            let _ = writeln!(schema, "  entity Effect;");
        } else {
            let values = self
                .effects
                .iter()
                .map(|effect| format!("{effect:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(schema, "  entity Effect enum [{values}];");
        }
        let base = "program: String, program_path: String, cwd: String, args: Set<String>, \
                    effects: Set<Effect>";
        let _ = writeln!(schema, "  type Output = {{ status: Long }};");
        let _ = writeln!(schema, "  action {ALL_ACTIONS:?};");
        let typed = self.grammar.action_fields();
        let unmodeled: (&str, Vec<(&str, FieldKind)>) = (UNMODELED_ACTION, Vec::new());
        for (action, fields) in typed.iter().chain(std::iter::once(&unmodeled)) {
            let mut record = base.to_string();
            for (name, kind) in fields {
                let _ = match kind {
                    FieldKind::Bool => write!(record, ", {name:?}: Bool"),
                    FieldKind::String => write!(record, ", {name:?}?: String"),
                    FieldKind::Set => write!(record, ", {name:?}: Set<String>"),
                };
            }
            let _ = writeln!(schema, "  type {action}Input = {{ {record} }};");
            let _ = writeln!(
                schema,
                "  action {action:?} in [{ALL_ACTIONS:?}] appliesTo {{\n    principal: [Box::Agent],\n    \
                 resource: [Box::Resource],\n    context: {{ input: {action}Input, output?: Output }}\n  }};"
            );
        }
        schema.push_str("}\n");
        schema
    }
}

/// One pack-action request, as the shell adapter hands it to the engine.
pub(crate) struct PackInvocation<'a> {
    pub(crate) pack: &'a Pack,
    pub(crate) parsed: &'a Parsed,
    pub(crate) program: &'a str,
    pub(crate) program_path: &'a str,
    pub(crate) args: &'a [String],
    pub(crate) cwd: &'a str,
}

impl PackInvocation<'_> {
    pub(crate) fn action(&self) -> &str {
        match self.parsed {
            Parsed::Action { action, .. } => action,
            Parsed::Unmodeled => UNMODELED_ACTION,
        }
    }

    /// The `context.input` record as Cedar's JSON form, entities in `__entity` escapes.
    pub(crate) fn input(&self) -> serde_json::Value {
        let effect_type = format!("{}::Effect", self.pack.namespace());
        let (fields, effects) = match self.parsed {
            Parsed::Action {
                fields, effects, ..
            } => (Some(fields), effects.iter().collect::<Vec<_>>()),
            Parsed::Unmodeled => (None, Vec::new()),
        };
        let mut args = self.args.to_vec();
        args.sort();
        args.dedup();
        let mut input = serde_json::json!({
            "program": self.program,
            "program_path": self.program_path,
            "cwd": self.cwd,
            "args": args,
            "effects": effects
                .into_iter()
                .map(|effect| serde_json::json!({ "__entity": { "type": effect_type, "id": effect } }))
                .collect::<Vec<_>>(),
        });
        if let (Some(fields), Some(object)) = (fields, input.as_object_mut()) {
            for (name, value) in fields {
                object.insert(name.clone(), field_json(value));
            }
        }
        input
    }
}

fn field_json(value: &FieldValue) -> serde_json::Value {
    match value {
        FieldValue::Bool(value) => serde_json::Value::Bool(*value),
        FieldValue::String(value) => serde_json::Value::String(value.clone()),
        FieldValue::Set(values) => serde_json::json!(values),
    }
}

/// What a permitted pack action needs to record its `::response` once the command ends.
#[derive(Debug, Clone)]
pub(crate) struct PackResponse {
    pub(crate) namespace: String,
    pub(crate) action: String,
    pub(crate) input: serde_json::Value,
}

impl PackInvocation<'_> {
    pub(crate) fn response(&self) -> PackResponse {
        PackResponse {
            namespace: self.pack.namespace(),
            action: self.action().to_string(),
            input: self.input(),
        }
    }
}

pub(crate) fn is_identifier(raw: &str) -> bool {
    !raw.is_empty() && cedar_identifier(raw) == raw
}

/// The installed packs, each bound to a resolved program file and the name it is called by.
#[derive(Debug, Clone, Default)]
pub struct PackBindings {
    packs: Vec<std::sync::Arc<Pack>>,
    by_program: std::collections::BTreeMap<(String, String), usize>,
    known_names: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

/// How a pack covers one spawn.
pub(crate) enum PackCoverage<'a> {
    /// The file and the name are bound to this pack: parse with its grammar.
    Parsed(&'a Pack),
    /// The file is bound to this pack under a name no `[tool]` declares.
    Unmodeled(&'a Pack),
}

impl PackBindings {
    /// Install `packs`, bound to no program yet.
    #[must_use]
    pub fn new(packs: Vec<Pack>) -> Self {
        Self {
            packs: packs.into_iter().map(std::sync::Arc::new).collect(),
            by_program: std::collections::BTreeMap::new(),
            known_names: std::collections::BTreeMap::new(),
        }
    }

    /// Declare that a `[tool]` runs the file at `program_path` under the name `name`.
    pub fn declare(&mut self, program_path: impl Into<String>, name: impl Into<String>) {
        self.known_names
            .entry(program_path.into())
            .or_default()
            .insert(name.into());
    }

    /// Bind the pack named `pack` to one resolved program file called by `name`.
    pub fn bind(&mut self, program_path: impl Into<String>, name: impl Into<String>, pack: &str) {
        if let Some(index) = self.packs.iter().position(|loaded| loaded.name() == pack) {
            let (program_path, name) = (program_path.into(), name.into());
            self.declare(program_path.clone(), name.clone());
            self.by_program.insert((program_path, name), index);
        }
    }

    /// Every installed pack.
    #[must_use]
    pub fn packs(&self) -> &[std::sync::Arc<Pack>] {
        &self.packs
    }

    /// The resolved program file, the name, and the pack of each binding.
    #[must_use]
    pub fn bound_programs(&self) -> Vec<(&str, &str, &Pack)> {
        self.by_program
            .iter()
            .filter_map(|((path, name), index)| {
                self.packs
                    .get(*index)
                    .map(|pack| (path.as_str(), name.as_str(), pack.as_ref()))
            })
            .collect()
    }

    pub(crate) fn schemas(&self) -> Vec<String> {
        self.packs.iter().map(|pack| pack.schema()).collect()
    }

    pub(crate) fn coverage(&self, program_path: &str, name: &str) -> Option<PackCoverage<'_>> {
        let pack = |index: &usize| self.packs.get(*index).map(std::convert::AsRef::as_ref);
        if let Some(index) = self
            .by_program
            .get(&(program_path.to_string(), name.to_string()))
        {
            return pack(index).map(PackCoverage::Parsed);
        }
        if self
            .known_names
            .get(program_path)
            .is_some_and(|names| names.contains(name))
        {
            return None;
        }
        self.by_program
            .iter()
            .find(|((path, _), _)| path == program_path)
            .and_then(|(_, index)| pack(index))
            .map(PackCoverage::Unmodeled)
    }
}
