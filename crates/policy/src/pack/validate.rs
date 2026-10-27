//! The content checks a tool pack file passes after it deserializes.

use std::collections::BTreeMap;

use super::PackRefusal;
use super::format::{OperandSpec, OptionSpec, PackFile, WhenRule};
use crate::schema::cedar_identifier;

/// Check the names, the program, the usage lines, and the effects of one pack file.
pub(super) fn check(file: &PackFile) -> Result<(), PackRefusal> {
    require_identifier("`pack`", &file.pack)?;
    if file.program.is_empty()
        || file.program.contains('/')
        || file.program.chars().any(char::is_whitespace)
    {
        return Err(PackRefusal::BadProgram(file.program.clone()));
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
    Ok(())
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
    use super::super::PackRefusal;
    use super::super::fixture::{refusal, with};

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
