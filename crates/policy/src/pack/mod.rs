//! Tool packs: an author-written action vocabulary and shell grammar for one program.
//!
//! | Module | Owns |
//! |---|---|
//! | `format` | the `*.pack.toml` document as written |

mod format;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use format::{GlobalSpec, OperandSpec, OptionSpec, PackFile, ShellActionSpec, WhenRule};

use crate::schema::cedar_identifier;

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
        require_identifier("`pack`", &file.pack)?;
        if file.program.is_empty()
            || file.program.contains('/')
            || file.program.chars().any(char::is_whitespace)
        {
            return Err(PackRefusal::BadProgram(file.program));
        }
        for effect in file.effects.keys() {
            require_identifier("effect", effect)?;
        }
        for (name, option) in &file.global.options {
            let place = format!("`[global] options.{name}`");
            require_identifier("`[global]` option", name)?;
            require_option_effects(&place, option, &file.effects)?;
        }
        for (action, spec) in &file.shell {
            require_identifier("action", action)?;
            let first = spec.usage.split_whitespace().next().unwrap_or_default();
            if first != file.program {
                return Err(PackRefusal::UsageDisagrees {
                    action: action.clone(),
                    first: first.to_string(),
                    program: file.program.clone(),
                });
            }
            require_declared(
                &format!("`[shell.{action}] adds`"),
                &spec.adds,
                &file.effects,
            )?;
            for (name, option) in &spec.options {
                let place = format!("`[shell.{action}] options.{name}`");
                require_identifier(&format!("`[shell.{action}]` option"), name)?;
                require_option_effects(&place, option, &file.effects)?;
            }
            for (name, operand) in &spec.operands {
                let place = format!("`[shell.{action}] operands.{name}`");
                require_identifier(&format!("`[shell.{action}]` operand"), name)?;
                require_operand_effects(&place, operand, &file.effects)?;
            }
        }

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

fn is_identifier(raw: &str) -> bool {
    !raw.is_empty() && cedar_identifier(raw) == raw
}

fn require_identifier(place: &str, name: &str) -> Result<(), PackRefusal> {
    if is_identifier(name) {
        Ok(())
    } else {
        Err(PackRefusal::NotIdentifier {
            place: place.to_string(),
            name: name.to_string(),
        })
    }
}

fn require_declared(
    place: &str,
    effects: &[String],
    declared: &BTreeMap<String, String>,
) -> Result<(), PackRefusal> {
    match effects
        .iter()
        .find(|effect| !declared.contains_key(*effect))
    {
        None => Ok(()),
        Some(effect) => Err(PackRefusal::UndeclaredEffect {
            place: place.to_string(),
            effect: effect.clone(),
        }),
    }
}

fn require_rule_effects(
    place: &str,
    adds: &[String],
    removes: &[String],
    when: &[WhenRule],
    declared: &BTreeMap<String, String>,
) -> Result<(), PackRefusal> {
    require_declared(place, adds, declared)?;
    require_declared(place, removes, declared)?;
    for rule in when {
        require_declared(place, &rule.adds, declared)?;
        require_declared(place, &rule.removes, declared)?;
    }
    Ok(())
}

fn require_option_effects(
    place: &str,
    option: &OptionSpec,
    declared: &BTreeMap<String, String>,
) -> Result<(), PackRefusal> {
    require_rule_effects(place, &option.adds, &option.removes, &option.when, declared)
}

fn require_operand_effects(
    place: &str,
    operand: &OperandSpec,
    declared: &BTreeMap<String, String>,
) -> Result<(), PackRefusal> {
    require_rule_effects(
        place,
        &operand.adds,
        &operand.removes,
        &operand.when,
        declared,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
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

    fn with(old: &str, new: &str) -> String {
        assert!(MINIMAL.contains(old), "the fixture holds {old:?}");
        MINIMAL.replacen(old, new, 1)
    }

    fn refusal(text: &str) -> PackRefusal {
        Pack::parse(text).expect_err("the pack is refused")
    }

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
        use format::{Flags, OptionValue, Patterns};

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

    #[test]
    fn a_pack_name_that_is_not_an_identifier_is_refused() {
        for name in ["git-lfs", "1git", "in", ""] {
            assert_eq!(
                refusal(&with("pack = \"git\"", &format!("pack = {name:?}"))),
                PackRefusal::NotIdentifier {
                    place: "`pack`".to_string(),
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
                place: "action".to_string(),
                name: "force-push".to_string(),
            }
        );
    }

    #[test]
    fn an_effect_name_that_is_not_an_identifier_is_refused() {
        assert_eq!(
            refusal(&with("force_push = \"", "\"force push\" = \"")),
            PackRefusal::NotIdentifier {
                place: "effect".to_string(),
                name: "force push".to_string(),
            }
        );
    }

    #[test]
    fn an_option_or_operand_name_that_is_not_an_identifier_is_refused() {
        assert!(matches!(
            refusal(&with("options.dry_run", "options.dry-run")),
            PackRefusal::NotIdentifier { name, .. } if name == "dry-run"
        ));
        assert!(matches!(
            refusal(&with("operands.refspec", "operands.\"ref spec\"")),
            PackRefusal::NotIdentifier { name, .. } if name == "ref spec"
        ));
        let global = with(
            "[shell.push]",
            "[global]\noptions.\"no-pager\" = { flag = \"-P\" }\n\n[shell.push]",
        );
        assert!(matches!(
            refusal(&global),
            PackRefusal::NotIdentifier { name, .. } if name == "no-pager"
        ));
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
                action: "push".to_string(),
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
        assert_eq!(
            refusal(&with(
                "removes = [\"force_push\"]",
                "removes = [\"forced\"]"
            )),
            PackRefusal::UndeclaredEffect {
                place: "`[shell.push] options.dry_run`".to_string(),
                effect: "forced".to_string(),
            }
        );
        assert!(matches!(
            refusal(&with("like = \"+*\", adds", "like = \"+*\", when = [{ like = \"*\", adds = [\"x\"] }], adds")),
            PackRefusal::UndeclaredEffect { effect, .. } if effect == "x"
        ));
        assert!(matches!(
            refusal(&with("options.force", "adds = [\"network\"]\noptions.force")),
            PackRefusal::UndeclaredEffect { effect, .. } if effect == "network"
        ));
        assert!(matches!(
            refusal(&with("like = \"+*\", adds", "like = \"+*\", when = [{ like = \"*\", removes = [\"y\"] }], adds")),
            PackRefusal::UndeclaredEffect { effect, .. } if effect == "y"
        ));
        let global = with(
            "[shell.push]",
            "[global]\noptions.pager = { flag = \"-p\", adds = [\"paged\"] }\n\n[shell.push]",
        );
        assert_eq!(
            refusal(&global),
            PackRefusal::UndeclaredEffect {
                place: "`[global] options.pager`".to_string(),
                effect: "paged".to_string(),
            }
        );
    }
}
