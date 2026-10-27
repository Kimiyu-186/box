//! Tool packs: an author-written action vocabulary and shell grammar for one program.
//!
//! | Module | Owns |
//! |---|---|
//! | `format` | the `*.pack.toml` document as written |
//! | `validate` | the content checks a document passes after it deserializes |

mod format;
mod validate;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use format::{GlobalSpec, PackFile, ShellActionSpec};

/// The pack file formats this Box reads.
const KNOWN_FORMATS: &[i64] = &[1];

/// A tool pack file that Box refuses to load.
#[derive(Debug, thiserror::Error)]
#[error("tool pack {}: {refusal}", .origin.display())]
pub(crate) struct PackError {
    origin: PathBuf,
    refusal: PackRefusal,
}

/// Why Box refuses a tool pack file.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PackRefusal {
    #[error("cannot read the file: {0}")]
    Unreadable(String),
    #[error("{0}")]
    Malformed(String),
    #[error("`format` is required: write `format = 1`")]
    MissingFormat,
    #[error("`format = {0}` is not a pack file format this Box reads (it reads 1)")]
    UnknownFormat(String),
    #[error("`version` is required: write `version = 1` for a new pack")]
    MissingVersion,
    #[error("`version = {0}` must be a positive integer")]
    BadVersion(String),
    #[error(
        "{place} {name:?} is not an identifier (ASCII letters, digits, and `_`, not first a digit)"
    )]
    NotIdentifier { place: String, name: String },
    #[error("`program = {0:?}` must be one program name, not a path")]
    BadProgram(String),
    #[error(
        "`[shell.{action}]` usage starts with {first:?}, but the pack's program is {program:?}"
    )]
    UsageDisagrees {
        action: String,
        first: String,
        program: String,
    },
    #[error("{place} names effect {effect:?}, which `[effects]` does not declare")]
    UndeclaredEffect { place: String, effect: String },
}

/// One tool pack file, read and checked.
#[derive(Debug)]
pub(crate) struct Pack {
    name: String,
    version: u32,
    program: String,
    effects: BTreeMap<String, String>,
    global: GlobalSpec,
    shell: BTreeMap<String, ShellActionSpec>,
}

impl Pack {
    /// Read and check the tool pack file at `path`.
    pub(crate) fn load(path: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(path).map_err(|error| PackError {
            origin: path.to_path_buf(),
            refusal: PackRefusal::Unreadable(error.to_string()),
        })?;
        Self::parse(&text).map_err(|refusal| PackError {
            origin: path.to_path_buf(),
            refusal,
        })
    }

    fn parse(text: &str) -> Result<Self, PackRefusal> {
        let malformed = |error: toml::de::Error| PackRefusal::Malformed(error.to_string());
        let document: toml::Table = toml::from_str(text).map_err(malformed)?;
        match document.get("format") {
            None => return Err(PackRefusal::MissingFormat),
            Some(toml::Value::Integer(format)) if KNOWN_FORMATS.contains(format) => {}
            Some(other) => return Err(PackRefusal::UnknownFormat(other.to_string())),
        }
        let file: PackFile = toml::from_str(text).map_err(malformed)?;

        let version = match &file.version {
            None => return Err(PackRefusal::MissingVersion),
            Some(toml::Value::Integer(version)) if *version >= 1 => {
                u32::try_from(*version).map_err(|_| PackRefusal::BadVersion(version.to_string()))?
            }
            Some(other) => return Err(PackRefusal::BadVersion(other.to_string())),
        };
        validate::check(&file)?;

        Ok(Self {
            name: file.pack,
            version,
            program: file.program,
            effects: file.effects,
            global: file.global,
            shell: file.shell,
        })
    }
}

#[cfg(test)]
mod fixture {
    use super::{Pack, PackRefusal};

    pub(super) const MINIMAL: &str = r#"
pack = "git"
program = "git"
format = 1
version = 1

[effects]
force_push = "Replaces commits on a remote branch"

[shell.push]
usage = "git [-C <directory>] push [options] [<remote> [<refspec>...]]"
options.force = { flag = ["-f", "--force"], adds = ["force_push"] }
options.dry_run = { flag = ["-n", "--dry-run"], removes = ["force_push"] }
operands.refspec = { like = "+*", adds = ["force_push"] }
"#;

    pub(super) fn with(old: &str, new: &str) -> String {
        assert!(MINIMAL.contains(old), "the fixture holds {old:?}");
        MINIMAL.replacen(old, new, 1)
    }

    pub(super) fn refusal(text: &str) -> PackRefusal {
        Pack::parse(text).expect_err("the pack is refused")
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{MINIMAL, refusal, with};
    use super::*;

    #[test]
    fn a_minimal_pack_loads_from_its_file() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("git.pack.toml");
        std::fs::write(&path, MINIMAL).expect("the pack file is written");

        let pack = Pack::load(&path).expect("the pack loads");

        assert_eq!(pack.name, "git");
        assert_eq!(pack.version, 1);
        assert_eq!(pack.program, "git");
        assert_eq!(pack.effects.keys().collect::<Vec<_>>(), ["force_push"]);
        assert_eq!(pack.shell.keys().collect::<Vec<_>>(), ["push"]);
        assert!(pack.global.options.is_empty());
    }

    #[test]
    fn every_nested_field_reads_into_its_own_place() {
        use format::{Flags, OperandSpec, OptionSpec, OptionValue, Patterns, WhenRule};

        let text = r#"
pack = "git"
program = "git"
format = 1
version = 2

[effects]
force_push = "Replaces commits on a remote branch"
network = "Contacts a remote repository"
override_config = "Overrides git configuration for this one command"

[global]
options.config = { flag = "-c", value = "required", repeated = true, adds = ["override_config"] }
options.directory = { flag = "-C", value = "many", anywhere = true }

[shell.push]
usage = "git [global options] push [options] [<remote> [<refspec>...]]"
aliases = ["send"]
adds = ["network"]
options.lease = { flag = ["--force-with-lease"], value = "optional", like = ["*:*", "*"], when = [{ like = "*", adds = ["force_push"], removes = ["network"] }] }
options.dry_run = { flag = ["-n", "--dry-run"], removes = ["force_push"] }
operands.refspec = { like = "+*", adds = ["force_push"], removes = ["network"], when = [{ like = ["a", "b"] }], rest = true }
"#;
        let strings = |values: &[&str]| values.iter().map(|value| value.to_string()).collect();
        let config = OptionSpec {
            flag: Flags::One("-c".to_string()),
            value: OptionValue::Required,
            repeated: true,
            like: None,
            adds: strings(&["override_config"]),
            removes: Vec::new(),
            when: Vec::new(),
            anywhere: false,
        };
        let directory = OptionSpec {
            flag: Flags::One("-C".to_string()),
            value: OptionValue::Many,
            repeated: false,
            like: None,
            adds: Vec::new(),
            removes: Vec::new(),
            when: Vec::new(),
            anywhere: true,
        };
        let lease = OptionSpec {
            flag: Flags::Many(strings(&["--force-with-lease"])),
            value: OptionValue::Optional,
            repeated: false,
            like: Some(Patterns::Many(strings(&["*:*", "*"]))),
            adds: Vec::new(),
            removes: Vec::new(),
            when: vec![WhenRule {
                like: Patterns::One("*".to_string()),
                adds: strings(&["force_push"]),
                removes: strings(&["network"]),
            }],
            anywhere: false,
        };
        let dry_run = OptionSpec {
            flag: Flags::Many(strings(&["-n", "--dry-run"])),
            value: OptionValue::None,
            repeated: false,
            like: None,
            adds: Vec::new(),
            removes: strings(&["force_push"]),
            when: Vec::new(),
            anywhere: false,
        };
        let refspec = OperandSpec {
            like: Some(Patterns::One("+*".to_string())),
            adds: strings(&["force_push"]),
            removes: strings(&["network"]),
            when: vec![WhenRule {
                like: Patterns::Many(strings(&["a", "b"])),
                adds: Vec::new(),
                removes: Vec::new(),
            }],
            rest: true,
        };
        let push = ShellActionSpec {
            usage: "git [global options] push [options] [<remote> [<refspec>...]]".to_string(),
            aliases: strings(&["send"]),
            adds: strings(&["network"]),
            options: BTreeMap::from([
                ("dry_run".to_string(), dry_run),
                ("lease".to_string(), lease),
            ]),
            operands: BTreeMap::from([("refspec".to_string(), refspec)]),
        };

        let pack = Pack::parse(text).expect("the pack loads");

        assert_eq!(pack.version, 2);
        assert_eq!(
            pack.effects,
            BTreeMap::from([
                (
                    "force_push".to_string(),
                    "Replaces commits on a remote branch".to_string()
                ),
                (
                    "network".to_string(),
                    "Contacts a remote repository".to_string()
                ),
                (
                    "override_config".to_string(),
                    "Overrides git configuration for this one command".to_string()
                ),
            ])
        );
        assert_eq!(
            pack.global,
            GlobalSpec {
                options: BTreeMap::from([
                    ("config".to_string(), config),
                    ("directory".to_string(), directory),
                ]),
            }
        );
        assert_eq!(pack.shell, BTreeMap::from([("push".to_string(), push)]));
    }

    #[test]
    fn a_refusal_names_the_file_and_the_reason() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("git.pack.toml");
        std::fs::write(&path, with("format = 1\n", "")).expect("the pack file is written");

        let message = Pack::load(&path)
            .expect_err("the pack is refused")
            .to_string();

        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("`format` is required"), "{message}");
    }

    #[test]
    fn a_missing_file_is_refused_as_unreadable() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let error = Pack::load(&directory.path().join("absent.pack.toml"))
            .expect_err("the pack is refused");

        assert!(matches!(error.refusal, PackRefusal::Unreadable(_)));
    }

    #[test]
    fn an_unknown_top_level_key_is_refused() {
        let PackRefusal::Malformed(reason) =
            refusal(&with("version = 1\n", "version = 1\npolicy = \"x\"\n"))
        else {
            panic!("an unknown key is a malformed document");
        };

        assert!(reason.contains("unknown field `policy`"), "{reason}");
    }

    #[test]
    fn an_unknown_key_inside_a_shell_table_is_refused() {
        let PackRefusal::Malformed(reason) =
            refusal(&with("[shell.push]\n", "[shell.push]\nprogram = \"git\"\n"))
        else {
            panic!("an unknown key is a malformed document");
        };

        assert!(reason.contains("unknown field `program`"), "{reason}");
    }

    #[test]
    fn a_missing_format_is_refused() {
        assert_eq!(
            refusal(&with("format = 1\n", "")),
            PackRefusal::MissingFormat
        );
    }

    #[test]
    fn an_unknown_format_is_refused() {
        assert_eq!(
            refusal(&with("format = 1\n", "format = 2\n")),
            PackRefusal::UnknownFormat("2".to_string())
        );
        assert_eq!(
            refusal(&with("format = 1\n", "format = \"1\"\n")),
            PackRefusal::UnknownFormat("\"1\"".to_string())
        );
    }

    #[test]
    fn an_unknown_format_is_named_before_an_unknown_key() {
        let text = with("format = 1\n", "format = 2\nhttp = {}\n");

        assert_eq!(refusal(&text), PackRefusal::UnknownFormat("2".to_string()));
    }

    #[test]
    fn a_missing_version_is_refused() {
        assert_eq!(
            refusal(&with("version = 1\n", "")),
            PackRefusal::MissingVersion
        );
    }

    #[test]
    fn a_version_of_zero_is_refused() {
        assert_eq!(
            refusal(&with("version = 1\n", "version = 0\n")),
            PackRefusal::BadVersion("0".to_string())
        );
    }

    #[test]
    fn a_negative_version_is_refused() {
        assert_eq!(
            refusal(&with("version = 1\n", "version = -1\n")),
            PackRefusal::BadVersion("-1".to_string())
        );
    }

    #[test]
    fn a_version_that_is_not_a_whole_number_is_refused() {
        for version in ["1.5", "\"2\"", "4294967296"] {
            assert!(
                matches!(
                    refusal(&with("version = 1\n", &format!("version = {version}\n"))),
                    PackRefusal::BadVersion(_)
                ),
                "version = {version}"
            );
        }
    }
}
