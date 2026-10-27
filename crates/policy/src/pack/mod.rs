//! Tool packs: an author-written action vocabulary and shell grammar for one program.
//!
//! | Module | Owns |
//! |---|---|
//! | `format` | the `*.pack.toml` document as written |
//! | `model` | the checked pack that loading builds from that document |

mod format;
mod model;

use std::fmt;
use std::path::{Path, PathBuf};

pub(crate) use model::Pack;

/// The pack file formats this Box reads.
const KNOWN_FORMATS: &[i64] = &[1];

/// A tool pack file that Box refuses to load.
#[derive(Debug, thiserror::Error)]
#[error("tool pack {}: {refusal}", .origin.display())]
pub(crate) struct PackError {
    origin: PathBuf,
    refusal: Box<PackRefusal>,
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
        "{place}: {name:?} is not an identifier (ASCII letters, digits, and `_`, not first a digit)"
    )]
    NotIdentifier { place: Place, name: String },
    #[error("`program = {0:?}` must be one program name, not a path")]
    BadProgram(String),
    #[error("{place}: usage starts with {first:?}, but the pack's program is {program:?}")]
    UsageDisagrees {
        place: Place,
        first: String,
        program: String,
    },
    #[error("{place}: names effect {effect:?}, which `[effects]` does not declare")]
    UndeclaredEffect { place: Place, effect: String },
}

/// One location in a tool pack file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Pack,
    Effect(String),
    GlobalOption(String),
    Action(String),
    ActionAdds(String),
    ActionOption { action: String, option: String },
    Operand { action: String, operand: String },
}

impl fmt::Display for Place {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pack => write!(formatter, "`pack`"),
            Self::Effect(effect) => write!(formatter, "`[effects] {effect}`"),
            Self::GlobalOption(option) => write!(formatter, "`[global] options.{option}`"),
            Self::Action(action) => write!(formatter, "`[shell.{action}]`"),
            Self::ActionAdds(action) => write!(formatter, "`[shell.{action}] adds`"),
            Self::ActionOption { action, option } => {
                write!(formatter, "`[shell.{action}] options.{option}`")
            }
            Self::Operand { action, operand } => {
                write!(formatter, "`[shell.{action}] operands.{operand}`")
            }
        }
    }
}

impl Pack {
    /// Read and check the tool pack file at `path`.
    pub(crate) fn load(path: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(path).map_err(|error| PackError {
            origin: path.to_path_buf(),
            refusal: Box::new(PackRefusal::Unreadable(error.to_string())),
        })?;
        Self::parse(&text).map_err(|refusal| PackError {
            origin: path.to_path_buf(),
            refusal: Box::new(refusal),
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
        Self::read(toml::from_str(text).map_err(malformed)?)
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
    use super::fixture::{refusal, with};
    use super::*;

    fn in_push(option: &str) -> Place {
        Place::ActionOption {
            action: "push".to_string(),
            option: option.to_string(),
        }
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

        assert!(matches!(*error.refusal, PackRefusal::Unreadable(_)));
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

    #[test]
    fn a_pack_name_that_is_not_an_identifier_is_refused() {
        for name in ["git-lfs", "1git", "in", ""] {
            assert_eq!(
                refusal(&with("pack = \"git\"", &format!("pack = {name:?}"))),
                PackRefusal::NotIdentifier {
                    place: Place::Pack,
                    name: name.to_string(),
                },
                "pack = {name:?}"
            );
        }
    }

    #[test]
    fn an_action_name_that_is_not_an_identifier_is_refused() {
        assert_eq!(
            refusal(&with("[shell.push]", "[shell.\"force-push\"]")),
            PackRefusal::NotIdentifier {
                place: Place::Action("force-push".to_string()),
                name: "force-push".to_string(),
            }
        );
    }

    #[test]
    fn an_effect_name_that_is_not_an_identifier_is_refused() {
        assert_eq!(
            refusal(&with("force_push = \"", "\"force push\" = \"")),
            PackRefusal::NotIdentifier {
                place: Place::Effect("force push".to_string()),
                name: "force push".to_string(),
            }
        );
    }

    #[test]
    fn an_option_or_operand_name_that_is_not_an_identifier_is_refused() {
        assert_eq!(
            refusal(&with("options.dry_run", "options.dry-run")),
            PackRefusal::NotIdentifier {
                place: in_push("dry-run"),
                name: "dry-run".to_string(),
            }
        );
        assert_eq!(
            refusal(&with("operands.refspec", "operands.\"ref spec\"")),
            PackRefusal::NotIdentifier {
                place: Place::Operand {
                    action: "push".to_string(),
                    operand: "ref spec".to_string(),
                },
                name: "ref spec".to_string(),
            }
        );
        let global = with(
            "[shell.push]",
            "[global]\noptions.\"no-pager\" = { flag = \"-P\" }\n\n[shell.push]",
        );
        assert_eq!(
            refusal(&global),
            PackRefusal::NotIdentifier {
                place: Place::GlobalOption("no-pager".to_string()),
                name: "no-pager".to_string(),
            }
        );
    }

    #[test]
    fn a_program_that_is_a_path_is_refused() {
        assert_eq!(
            refusal(&with("program = \"git\"", "program = \"/usr/bin/git\"")),
            PackRefusal::BadProgram("/usr/bin/git".to_string())
        );
    }

    #[test]
    fn a_usage_that_does_not_start_with_the_program_is_refused() {
        assert_eq!(
            refusal(&with("usage = \"git ", "usage = \"hub ")),
            PackRefusal::UsageDisagrees {
                place: Place::Action("push".to_string()),
                first: "hub".to_string(),
                program: "git".to_string(),
            }
        );
        assert!(matches!(
            refusal(&with("usage = \"git [-C", "usage = \"  [-C")),
            PackRefusal::UsageDisagrees { first, .. } if first == "[-C"
        ));
    }

    #[test]
    fn an_effect_that_effects_does_not_declare_is_refused() {
        let undeclared = |place, effect: &str| PackRefusal::UndeclaredEffect {
            place,
            effect: effect.to_string(),
        };
        let refspec = || Place::Operand {
            action: "push".to_string(),
            operand: "refspec".to_string(),
        };
        let when = |key: &str, effect: &str| {
            let entry =
                format!("like = \"+*\", when = [{{ like = \"*\", {key} = [\"{effect}\"] }}], adds");
            with("like = \"+*\", adds", &entry)
        };

        assert_eq!(
            refusal(&with(
                "removes = [\"force_push\"]",
                "removes = [\"forced\"]"
            )),
            undeclared(in_push("dry_run"), "forced")
        );
        assert_eq!(refusal(&when("adds", "x")), undeclared(refspec(), "x"));
        assert_eq!(refusal(&when("removes", "y")), undeclared(refspec(), "y"));
        assert_eq!(
            refusal(&with(
                "options.force",
                "adds = [\"network\"]\noptions.force"
            )),
            undeclared(Place::ActionAdds("push".to_string()), "network")
        );
        let global = with(
            "[shell.push]",
            "[global]\noptions.pager = { flag = \"-p\", adds = [\"paged\"] }\n\n[shell.push]",
        );
        assert_eq!(
            refusal(&global),
            undeclared(Place::GlobalOption("pager".to_string()), "paged")
        );
    }

    #[test]
    fn an_option_inside_a_shell_action_is_named_one_way_in_every_refusal() {
        let named = refusal(&with("options.dry_run", "options.dry-run")).to_string();
        let effect = refusal(&with(
            "removes = [\"force_push\"]",
            "removes = [\"forced\"]",
        ))
        .to_string();

        assert_eq!(
            in_push("dry_run").to_string(),
            "`[shell.push] options.dry_run`"
        );
        assert!(
            named.starts_with("`[shell.push] options.dry-run`: "),
            "{named}"
        );
        assert!(
            effect.starts_with("`[shell.push] options.dry_run`: "),
            "{effect}"
        );
    }
}
