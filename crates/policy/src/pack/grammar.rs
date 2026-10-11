//! The shell grammar: one command line in, one pack action out.
//!
//! This parser is literal: an action matches when the arguments start with the subcommand words
//! of its `usage` line (the last word may be spelled as one of its `aliases`). Options, operands,
//! and effects are not read yet, so `fields` and `effects` are always empty.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One typed `context.input` field value.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldValue {
    Bool(bool),
    String(String),
    Set(Vec<String>),
}

/// What the grammar made of one command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parsed {
    Action {
        action: String,
        fields: BTreeMap<String, FieldValue>,
        effects: BTreeSet<String>,
    },
    Unmodeled,
}

/// One action's literal recognizer.
#[derive(Debug, Clone)]
struct ActionGrammar {
    action: String,
    /// The literal subcommand words, each with its accepted spellings.
    words: Vec<Vec<String>>,
}

/// Every shell action of one pack.
#[derive(Debug, Clone)]
pub(crate) struct ShellGrammar {
    actions: Vec<ActionGrammar>,
}

impl ShellGrammar {
    pub(crate) fn new(
        actions: impl IntoIterator<Item = (String, Vec<String>, Vec<String>)>,
    ) -> Self {
        let mut actions = actions
            .into_iter()
            .map(|(action, words, aliases)| {
                let count = words.len();
                let words = words
                    .into_iter()
                    .enumerate()
                    .map(|(index, word)| {
                        let mut spellings = vec![word];
                        if index + 1 == count {
                            spellings.extend(aliases.iter().cloned());
                        }
                        spellings
                    })
                    .collect();
                ActionGrammar { action, words }
            })
            .collect::<Vec<_>>();
        // The longest subcommand wins, so `stash pop` is tried before `stash`.
        actions.sort_by_key(|grammar| std::cmp::Reverse(grammar.words.len()));
        Self { actions }
    }

    /// Recognize `argv`, the arguments after the program.
    pub(crate) fn parse(&self, argv: &[String], _cwd: &Path) -> Parsed {
        for grammar in &self.actions {
            if grammar.words.len() > argv.len() {
                continue;
            }
            if grammar
                .words
                .iter()
                .zip(argv)
                .all(|(spellings, argument)| spellings.iter().any(|word| word == argument))
            {
                return Parsed::Action {
                    action: grammar.action.clone(),
                    fields: BTreeMap::new(),
                    effects: BTreeSet::new(),
                };
            }
        }
        Parsed::Unmodeled
    }
}

/// The literal subcommand words of a usage line, after the program: each top-level word that is
/// not an option, a placeholder, or a bracketed group, up to the first one that is not literal
/// after a literal was seen.
pub(crate) fn subcommand_words(usage: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut depth = 0usize;
    for token in usage.split_whitespace().skip(1) {
        let opens = token
            .chars()
            .filter(|c| matches!(c, '[' | '{' | '('))
            .count();
        let closes = token
            .chars()
            .filter(|c| matches!(c, ']' | '}' | ')'))
            .count();
        let top_level = depth == 0 && opens == 0;
        depth = (depth + opens).saturating_sub(closes);
        if !top_level {
            if !words.is_empty() {
                break;
            }
            continue;
        }
        let literal = !token.starts_with('-')
            && !token.starts_with('<')
            && !token.contains("...")
            && token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
        if literal {
            words.push(token.to_string());
        } else if !words.is_empty() {
            break;
        }
    }
    words
}
