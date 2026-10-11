//! The shell grammar: one command line in, one pack action out.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::format::{OperandSpec, OptionSpec, OptionValue, PackFile, Patterns, WhenRule};

/// One typed `context.input` field value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldValue {
    Bool(bool),
    String(String),
    Set(Vec<String>),
}

/// The Cedar type of one generated field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Bool,
    String,
    Set,
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

/// The input field names every pack action carries.
pub(crate) const BASE_FIELDS: [&str; 5] = ["program", "program_path", "cwd", "args", "effects"];

#[derive(Debug, Clone, Default)]
struct EffectRules {
    like: Vec<String>,
    adds: Vec<String>,
    removes: Vec<String>,
    when: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Rule {
    like: Vec<String>,
    adds: Vec<String>,
    removes: Vec<String>,
}

#[derive(Debug, Clone)]
struct OptionDef {
    name: String,
    value: OptionValue,
    repeated: bool,
    anywhere: bool,
    effects: EffectRules,
    /// `None` for a `[global]` option, else the actions whose usage declares it inline.
    inline_for: Option<BTreeSet<usize>>,
}

impl OptionDef {
    fn kind(&self) -> FieldKind {
        match self.value {
            OptionValue::None => FieldKind::Bool,
            OptionValue::Many => FieldKind::Set,
            _ if self.repeated => FieldKind::Set,
            _ => FieldKind::String,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct OptionTable {
    defs: Vec<OptionDef>,
    flags: BTreeMap<String, usize>,
}

impl OptionTable {
    fn insert(&mut self, flags: &[String], def: OptionDef) -> Result<(), String> {
        let index = self.defs.len();
        for flag in flags {
            if self.flags.insert(flag.clone(), index).is_some() {
                return Err(format!("flag {flag} is declared twice"));
            }
        }
        self.defs.push(def);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct OperandDef {
    name: String,
    required: bool,
    variadic: bool,
    rest: bool,
    effects: EffectRules,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Separator {
    #[default]
    None,
    Optional(usize),
    Required(usize),
}

#[derive(Debug, Clone)]
struct FieldDef {
    name: String,
    kind: FieldKind,
    source: FieldSource,
}

#[derive(Debug, Clone)]
enum FieldSource {
    Option(EffectRules),
    Operand(usize),
    Toolchain,
}

/// One action's recognizer.
#[derive(Debug, Clone)]
struct ActionGrammar {
    action: String,
    spellings: Vec<Vec<String>>,
    globals: bool,
    toolchain: Option<String>,
    adds: Vec<String>,
    post: OptionTable,
    between: OptionTable,
    operands: Vec<OperandDef>,
    separator: Separator,
    fields: Vec<FieldDef>,
}

/// Every shell action of one pack.
#[derive(Debug, Clone)]
pub(crate) struct ShellGrammar {
    pre: OptionTable,
    toolchain: bool,
    actions: Vec<ActionGrammar>,
}

/// One node of a tokenized usage line.
#[derive(Debug, Clone)]
enum Node {
    Word(String),
    Group(Vec<Node>),
}

fn tokenize(usage: &str) -> Result<Vec<Node>, String> {
    let mut stack: Vec<Vec<Node>> = vec![Vec::new()];
    let mut word = String::new();
    let flush = |word: &mut String, stack: &mut Vec<Vec<Node>>| {
        if !word.is_empty()
            && let Some(top) = stack.last_mut()
        {
            top.push(Node::Word(std::mem::take(word)));
        }
    };
    for character in usage.chars() {
        match character {
            '[' => {
                flush(&mut word, &mut stack);
                stack.push(Vec::new());
            }
            ']' => {
                flush(&mut word, &mut stack);
                let group = stack.pop().ok_or("unbalanced ']'")?;
                stack
                    .last_mut()
                    .ok_or("unbalanced ']'")?
                    .push(Node::Group(group));
            }
            '|' | '(' | ')' | '{' | '}' => {
                return Err(format!("{character:?} is not supported in a usage line"));
            }
            character if character.is_whitespace() => flush(&mut word, &mut stack),
            character => word.push(character),
        }
    }
    flush(&mut word, &mut stack);
    if stack.len() != 1 {
        return Err("unbalanced '['".to_string());
    }
    Ok(stack.pop().unwrap_or_default())
}

fn is_literal(word: &str) -> bool {
    let mut characters = word.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn placeholder(word: &str) -> Option<(&str, bool)> {
    let (inner, variadic) = match word.strip_suffix("...") {
        Some(inner) => (inner, true),
        None => (word, false),
    };
    let name = inner.strip_prefix('<')?.strip_suffix('>')?;
    super::is_identifier(name).then_some((name, variadic))
}

fn valid_flag(flag: &str) -> bool {
    if let Some(long) = flag.strip_prefix("--") {
        is_literal(long)
    } else if let Some(short) = flag.strip_prefix('-') {
        let mut characters = short.chars();
        matches!((characters.next(), characters.next()), (Some(c), None) if c.is_ascii_alphanumeric())
    } else {
        false
    }
}

/// What one usage line declares, before it is joined with the option tables.
#[derive(Default)]
struct UsageShape {
    words: Vec<String>,
    globals: bool,
    toolchain: Option<String>,
    options_placeholder: bool,
    inline_pre: Vec<(Vec<String>, OptionDef)>,
    inline_post: Vec<(Vec<String>, OptionDef)>,
    operands: Vec<(String, bool, bool)>,
    separator: Separator,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Before,
    Words,
    After,
}

struct UsageWalker<'a> {
    shape: UsageShape,
    phase: Phase,
    position: usize,
    known_flags: &'a BTreeSet<String>,
}

impl UsageWalker<'_> {
    fn walk(&mut self, nodes: &[Node], optional: bool) -> Result<(), String> {
        let mut index = 0;
        while index < nodes.len() {
            let node = &nodes[index];
            index += 1;
            self.position += 1;
            match node {
                Node::Word(word) => self.word(word, optional)?,
                Node::Group(group) => self.group(group)?,
            }
        }
        Ok(())
    }

    fn after(&mut self) -> Result<(), String> {
        if self.phase == Phase::Before {
            return Err("an operand or separator comes before the command words".to_string());
        }
        self.phase = Phase::After;
        Ok(())
    }

    fn word(&mut self, word: &str, optional: bool) -> Result<(), String> {
        if word == "--" {
            return self.separator(true);
        }
        if let Some((name, variadic)) = placeholder(word) {
            self.after()?;
            self.shape
                .operands
                .push((name.to_string(), !optional, variadic));
            return Ok(());
        }
        if is_literal(word) {
            if optional || self.phase == Phase::After {
                return Err(format!(
                    "command word {word:?} must be required and contiguous with the others"
                ));
            }
            self.phase = Phase::Words;
            self.shape.words.push(word.to_string());
            return Ok(());
        }
        Err(format!(
            "{word:?} is not a word, an operand, or a separator"
        ))
    }

    fn separator(&mut self, required: bool) -> Result<(), String> {
        self.after()?;
        if self.shape.separator != Separator::None {
            return Err("a usage line has at most one `--`".to_string());
        }
        let at = self.shape.operands.len();
        self.shape.separator = if required {
            Separator::Required(at)
        } else {
            Separator::Optional(at)
        };
        Ok(())
    }

    fn group(&mut self, group: &[Node]) -> Result<(), String> {
        let words: Vec<&str> = group
            .iter()
            .map(|node| match node {
                Node::Word(word) => Some(word.as_str()),
                Node::Group(_) => None,
            })
            .take_while(Option::is_some)
            .flatten()
            .collect();
        match (words.as_slice(), group.len()) {
            (["global", "options"], 2) => {
                if self.phase != Phase::Before {
                    return Err("`[global options]` must come before the command words".into());
                }
                self.shape.globals = true;
                Ok(())
            }
            (["options"], 1) => {
                if self.phase == Phase::Before {
                    return Err("`[options]` must come after the command words".into());
                }
                self.phase = Phase::After;
                self.shape.options_placeholder = true;
                Ok(())
            }
            (["--"], 1) => self.separator(false),
            ([first, ..], _) if *first == "--" => {
                self.separator(true)?;
                self.walk(&group[1..], true)
            }
            ([first], 1) if first.starts_with("+<") && first.ends_with('>') => {
                let name = &first[2..first.len() - 1];
                if self.position != 1 || !super::is_identifier(name) {
                    return Err("`[+<toolchain>]` must directly follow the program".into());
                }
                self.shape.toolchain = Some(name.to_string());
                Ok(())
            }
            ([first, ..], _) if first.starts_with('-') => self.inline_option(group),
            _ => self.walk(group, true),
        }
    }

    fn inline_option(&mut self, group: &[Node]) -> Result<(), String> {
        let Some(Node::Word(raw)) = group.first() else {
            return Err("an inline option has no flag".into());
        };
        let (flag, mut value, mut name, mut repeated) = match raw.split_once("=<") {
            Some((flag, rest)) => {
                let name = rest
                    .strip_suffix('>')
                    .ok_or("bad `=<name>` in an inline option")?;
                (
                    flag.to_string(),
                    OptionValue::Required,
                    Some(name.to_string()),
                    false,
                )
            }
            None => (raw.clone(), OptionValue::None, None, false),
        };
        match group.get(1..).unwrap_or_default() {
            [] => {}
            [Node::Word(word)] if name.is_none() => {
                let (placeholder, variadic) =
                    placeholder(word).ok_or("an inline option value must be `<name>`")?;
                value = OptionValue::Required;
                name = Some(placeholder.to_string());
                repeated = variadic;
            }
            [Node::Group(inner)] if name.is_none() => match inner.as_slice() {
                [Node::Word(word)] if word.starts_with("=<") && word.ends_with('>') => {
                    value = OptionValue::Optional;
                    name = Some(word[2..word.len() - 1].to_string());
                }
                _ => return Err("bad optional value in an inline option".into()),
            },
            _ => return Err(format!("cannot read the inline option {raw:?}")),
        }
        if !valid_flag(&flag) {
            return Err(format!("{flag:?} is not a valid flag"));
        }
        if self.known_flags.contains(&flag) {
            return Ok(());
        }
        let name = match name {
            Some(name) => name,
            None => flag
                .strip_prefix("--")
                .ok_or_else(|| format!("bare short flag {flag} needs an `options` entry"))?
                .replace('-', "_"),
        };
        if !super::is_identifier(&name) {
            return Err(format!("inline option field {name:?} is not an identifier"));
        }
        let def = OptionDef {
            name,
            value,
            repeated,
            anywhere: false,
            effects: EffectRules::default(),
            inline_for: Some(BTreeSet::new()),
        };
        if self.phase == Phase::Before {
            self.shape.inline_pre.push((vec![flag], def));
        } else {
            self.phase = Phase::After;
            self.shape.inline_post.push((vec![flag], def));
        }
        Ok(())
    }
}

fn rules(when: &[WhenRule]) -> Vec<Rule> {
    when.iter()
        .map(|rule| Rule {
            like: rule.like.list(),
            adds: rule.adds.clone(),
            removes: rule.removes.clone(),
        })
        .collect()
}

fn option_def(name: &str, spec: &OptionSpec, global: bool) -> Result<OptionDef, String> {
    let owner = format!("options.{name}");
    if spec.flag.spellings().is_empty() {
        return Err(format!("{owner} has no flag"));
    }
    for flag in spec.flag.spellings() {
        if !valid_flag(&flag) {
            return Err(format!(
                "{owner} flag {flag:?} must be `-x` or `--long-name`, and never carry `=`"
            ));
        }
    }
    if spec.value == OptionValue::None {
        if spec.like.is_some() || !spec.when.is_empty() {
            return Err(format!(
                "{owner} is a Bool flag, so it cannot carry `like` or `when`"
            ));
        }
        if spec.repeated {
            return Err(format!(
                "{owner} is a Bool flag, so `repeated` means nothing"
            ));
        }
    }
    if spec.anywhere && !global {
        return Err(format!("{owner}: `anywhere` belongs in `[global]` only"));
    }
    Ok(OptionDef {
        name: name.to_string(),
        value: spec.value,
        repeated: spec.repeated,
        anywhere: spec.anywhere,
        effects: EffectRules {
            like: spec.like.as_ref().map(Patterns::list).unwrap_or_default(),
            adds: spec.adds.clone(),
            removes: spec.removes.clone(),
            when: rules(&spec.when),
        },
        inline_for: (!global).then(BTreeSet::new),
    })
}

fn operand_rules(spec: &OperandSpec) -> EffectRules {
    EffectRules {
        like: spec.like.as_ref().map(Patterns::list).unwrap_or_default(),
        adds: spec.adds.clone(),
        removes: spec.removes.clone(),
        when: rules(&spec.when),
    }
}

fn check_effects(
    rules: &EffectRules,
    declared: &BTreeSet<&str>,
    owner: &str,
) -> Result<(), String> {
    for effect in rules.adds.iter().chain(&rules.removes).chain(
        rules
            .when
            .iter()
            .flat_map(|rule| rule.adds.iter().chain(&rule.removes)),
    ) {
        if !declared.contains(effect.as_str()) {
            return Err(format!(
                "{owner} names effect {effect:?}, which `[effects]` does not declare"
            ));
        }
    }
    Ok(())
}

impl ShellGrammar {
    /// Compile every `[shell.*]` table of one pack, refusing anything the parser cannot read.
    pub(crate) fn compile(file: &PackFile) -> Result<Self, String> {
        let declared: BTreeSet<&str> = file.effects.keys().map(String::as_str).collect();

        let mut globals = OptionTable::default();
        for (name, spec) in &file.global.options {
            if !super::is_identifier(name) {
                return Err(format!("`[global] options.{name}` is not an identifier"));
            }
            let def = option_def(name, spec, true).map_err(|e| format!("`[global]` {e}"))?;
            check_effects(
                &def.effects,
                &declared,
                &format!("`[global] options.{name}`"),
            )?;
            globals
                .insert(&spec.flag.spellings(), def)
                .map_err(|e| format!("`[global]` {e}"))?;
        }

        let mut pre = globals.clone();
        let mut actions = Vec::new();
        let mut seen_spellings: BTreeMap<Vec<String>, String> = BTreeMap::new();

        for (index, (action, spec)) in file.shell.iter().enumerate() {
            let fail = |reason: String| format!("`[shell.{action}]` {reason}");
            let nodes = tokenize(&spec.usage).map_err(&fail)?;
            let Some((Node::Word(first), rest)) = nodes.split_first() else {
                return Err(fail("usage must start with the program".into()));
            };
            if *first != file.program {
                return Err(fail(format!(
                    "usage starts with {first:?}, but the pack's program is {:?}",
                    file.program
                )));
            }
            let mut known: BTreeSet<String> = globals.flags.keys().cloned().collect();
            for option in spec.options.values() {
                known.extend(option.flag.spellings());
            }
            let mut walker = UsageWalker {
                shape: UsageShape::default(),
                phase: Phase::Before,
                position: 0,
                known_flags: &known,
            };
            walker.walk(rest, false).map_err(&fail)?;
            let shape = walker.shape;
            if shape.words.is_empty() {
                return Err(fail("usage names no command word".into()));
            }
            if !spec.options.is_empty() && !shape.options_placeholder {
                return Err(fail(
                    "declares `options` but its usage has no `[options]`".into(),
                ));
            }
            check_effects(
                &EffectRules {
                    adds: spec.adds.clone(),
                    ..EffectRules::default()
                },
                &declared,
                &fail("adds".into()),
            )?;

            let mut spellings = vec![shape.words.clone()];
            for alias in &spec.aliases {
                let words: Vec<String> = alias.split_whitespace().map(str::to_string).collect();
                if words.is_empty() || !words.iter().all(|word| is_literal(word)) {
                    return Err(fail(format!("alias {alias:?} must be literal words")));
                }
                spellings.push(words);
            }
            for spelling in &spellings {
                if let Some(other) = seen_spellings.insert(spelling.clone(), action.clone()) {
                    return Err(fail(format!(
                        "spells `{}`, as `[shell.{other}]` does",
                        spelling.join(" ")
                    )));
                }
            }

            let mut fields: Vec<FieldDef> = Vec::new();
            let mut post = OptionTable::default();
            let mut between = OptionTable::default();
            if shape.globals {
                for def in &globals.defs {
                    fields.push(FieldDef {
                        name: def.name.clone(),
                        kind: def.kind(),
                        source: FieldSource::Option(def.effects.clone()),
                    });
                }
                for (flag, def_index) in &globals.flags {
                    let def = &globals.defs[*def_index];
                    if def.anywhere {
                        let mut copy = def.clone();
                        copy.inline_for = None;
                        for table in [&mut post, &mut between] {
                            match table.defs.iter().position(|d| d.name == def.name) {
                                Some(at) => {
                                    table.flags.insert(flag.clone(), at);
                                }
                                None => table.insert(std::slice::from_ref(flag), copy.clone())?,
                            }
                        }
                    }
                }
            }
            for (flags, mut def) in shape.inline_pre {
                let flag = &flags[0];
                match pre.flags.get(flag) {
                    Some(at) => {
                        let existing = &mut pre.defs[*at];
                        match &mut existing.inline_for {
                            Some(owners)
                                if existing.name == def.name && existing.value == def.value =>
                            {
                                owners.insert(index);
                            }
                            _ => {
                                return Err(fail(format!(
                                    "inline option {flag} conflicts with another declaration"
                                )));
                            }
                        }
                    }
                    None => {
                        def.inline_for = Some(BTreeSet::from([index]));
                        fields.push(FieldDef {
                            name: def.name.clone(),
                            kind: def.kind(),
                            source: FieldSource::Option(EffectRules::default()),
                        });
                        pre.insert(&flags, def).map_err(&fail)?;
                        continue;
                    }
                }
                fields.push(FieldDef {
                    name: def.name.clone(),
                    kind: def.kind(),
                    source: FieldSource::Option(EffectRules::default()),
                });
            }
            for (name, option) in &spec.options {
                if !super::is_identifier(name) {
                    return Err(fail(format!("options.{name} is not an identifier")));
                }
                let def = option_def(name, option, false).map_err(&fail)?;
                check_effects(&def.effects, &declared, &fail(format!("options.{name}")))?;
                fields.push(FieldDef {
                    name: def.name.clone(),
                    kind: def.kind(),
                    source: FieldSource::Option(def.effects.clone()),
                });
                post.insert(&option.flag.spellings(), def)
                    .map_err(|e| fail(format!("{e} (an `anywhere` global or another option)")))?;
            }
            for (flags, def) in shape.inline_post {
                fields.push(FieldDef {
                    name: def.name.clone(),
                    kind: def.kind(),
                    source: FieldSource::Option(EffectRules::default()),
                });
                post.insert(&flags, def).map_err(&fail)?;
            }

            let mut operands = Vec::new();
            for (name, required, variadic) in &shape.operands {
                let operand = spec.operands.get(name).cloned().unwrap_or_default();
                let rules = operand_rules(&operand);
                check_effects(&rules, &declared, &fail(format!("operands.{name}")))?;
                if operand.rest && shape.operands.len() != 1 {
                    return Err(fail(format!(
                        "operands.{name}: `rest` needs to be the only operand"
                    )));
                }
                fields.push(FieldDef {
                    name: name.clone(),
                    kind: if *variadic || operand.rest {
                        FieldKind::Set
                    } else {
                        FieldKind::String
                    },
                    source: FieldSource::Operand(operands.len()),
                });
                operands.push(OperandDef {
                    name: name.clone(),
                    required: *required,
                    variadic: *variadic || operand.rest,
                    rest: operand.rest,
                    effects: rules,
                });
            }
            for name in spec.operands.keys() {
                if !operands.iter().any(|operand| &operand.name == name) {
                    return Err(fail(format!("operands.{name} does not appear in usage")));
                }
            }
            if let Some(toolchain) = &shape.toolchain {
                fields.push(FieldDef {
                    name: toolchain.clone(),
                    kind: FieldKind::String,
                    source: FieldSource::Toolchain,
                });
            }
            let mut names: BTreeSet<&str> = BASE_FIELDS.into_iter().collect();
            for field in &fields {
                if !names.insert(field.name.as_str()) {
                    return Err(fail(format!(
                        "field {:?} is declared twice, or is reserved",
                        field.name
                    )));
                }
            }

            actions.push(ActionGrammar {
                action: action.clone(),
                spellings,
                globals: shape.globals,
                toolchain: shape.toolchain,
                adds: spec.adds.clone(),
                post,
                between,
                operands,
                separator: shape.separator,
                fields,
            });
        }

        Ok(Self {
            toolchain: actions.iter().any(|action| action.toolchain.is_some()),
            pre,
            actions,
        })
    }

    /// The typed fields of each action, in declaration order.
    pub(crate) fn action_fields(&self) -> Vec<(&str, Vec<(&str, FieldKind)>)> {
        self.actions
            .iter()
            .map(|action| {
                (
                    action.action.as_str(),
                    action
                        .fields
                        .iter()
                        .map(|field| (field.name.as_str(), field.kind))
                        .collect(),
                )
            })
            .collect()
    }

    /// Recognize `argv`, the arguments after the program.
    pub(crate) fn parse(&self, argv: &[String], _cwd: &Path) -> Parsed {
        self.read(argv).unwrap_or(Parsed::Unmodeled)
    }

    fn read(&self, argv: &[String]) -> Option<Parsed> {
        let mut seen = Occurrences::default();
        let mut index = 0;
        let mut toolchain = None;
        if self.toolchain
            && let Some(selector) = argv.first().and_then(|first| first.strip_prefix('+'))
        {
            toolchain = Some(selector.to_string());
            index = 1;
        }
        let mut pre_used = Vec::new();
        loop {
            let word = argv.get(index)?;
            if word == "--" {
                return None;
            }
            if !is_option_word(word) {
                break;
            }
            index = read_option(&self.pre, argv, index, &mut seen, &mut pre_used)?;
        }

        let (action_index, mut index, mut seen) = self.match_words(argv, index, &seen)?;
        let action = &self.actions[action_index];
        for used in pre_used {
            let allowed = match &self.pre.defs[used].inline_for {
                None => action.globals,
                Some(owners) => owners.contains(&action_index),
            };
            if !allowed {
                return None;
            }
        }
        if toolchain.is_some() && action.toolchain.is_none() {
            return None;
        }

        let rest = action.operands.first().is_some_and(|operand| operand.rest);
        let mut before: Vec<String> = Vec::new();
        let mut after: Vec<String> = Vec::new();
        let mut separated = false;
        let mut verbatim = false;
        let mut ignored = Vec::new();
        while let Some(word) = argv.get(index) {
            if verbatim {
                before.push(word.clone());
                index += 1;
            } else if separated {
                after.push(word.clone());
                index += 1;
            } else if word == "--" {
                if rest {
                    verbatim = true;
                } else {
                    separated = true;
                }
                index += 1;
            } else if is_option_word(word) {
                index = read_option(&action.post, argv, index, &mut seen, &mut ignored)?;
            } else {
                verbatim = rest;
                before.push(word.clone());
                index += 1;
            }
        }

        let filled = fill(action, before, after, separated)?;
        let mut fields = BTreeMap::new();
        let mut adds: BTreeSet<String> = action.adds.iter().cloned().collect();
        let mut removes = BTreeSet::new();
        for field in &action.fields {
            let values: Option<Vec<String>> = match &field.source {
                FieldSource::Option(_) => seen.values.get(&field.name).cloned(),
                FieldSource::Operand(at) => {
                    Some(filled[*at].clone()).filter(|words| !words.is_empty())
                }
                FieldSource::Toolchain => toolchain.clone().map(|selector| vec![selector]),
            };
            let value = match (field.kind, &values) {
                (FieldKind::Bool, _) => Some(FieldValue::Bool(values.is_some())),
                (FieldKind::Set, _) => {
                    let mut set = values.clone().unwrap_or_default();
                    set.sort();
                    set.dedup();
                    Some(FieldValue::Set(set))
                }
                (FieldKind::String, Some(values)) => {
                    values.first().cloned().map(FieldValue::String)
                }
                (FieldKind::String, None) => None,
            };
            if let Some(value) = value {
                fields.insert(field.name.clone(), value);
            }
            let rules = match &field.source {
                FieldSource::Option(rules) => rules,
                FieldSource::Operand(at) => &action.operands[*at].effects,
                FieldSource::Toolchain => continue,
            };
            if let Some(values) = &values {
                apply(rules, values, &mut adds, &mut removes);
            }
        }
        let effects = adds.difference(&removes).cloned().collect();
        Some(Parsed::Action {
            action: action.action.clone(),
            fields,
            effects,
        })
    }

    /// The longest action whose command words start at `start`, with the state after them.
    fn match_words(
        &self,
        argv: &[String],
        start: usize,
        seen: &Occurrences,
    ) -> Option<(usize, usize, Occurrences)> {
        let mut best: Option<(usize, usize, usize, Occurrences)> = None;
        for (action_index, action) in self.actions.iter().enumerate() {
            for spelling in &action.spellings {
                let mut state = seen.clone();
                let mut index = start;
                let mut matched = true;
                for (position, word) in spelling.iter().enumerate() {
                    if position > 0 {
                        while let Some(next) = argv.get(index)
                            && is_option_word(next)
                        {
                            let mut ignored = Vec::new();
                            match read_option(
                                &action.between,
                                argv,
                                index,
                                &mut state,
                                &mut ignored,
                            ) {
                                Some(next) => index = next,
                                None => {
                                    matched = false;
                                    break;
                                }
                            }
                        }
                    }
                    if !matched || argv.get(index) != Some(word) {
                        matched = false;
                        break;
                    }
                    index += 1;
                }
                if matched
                    && best
                        .as_ref()
                        .is_none_or(|(_, _, len, _)| spelling.len() > *len)
                {
                    best = Some((action_index, index, spelling.len(), state));
                }
            }
        }
        best.map(|(action, index, _, state)| (action, index, state))
    }
}

fn is_option_word(word: &str) -> bool {
    word.starts_with('-') && word.len() > 1 && word != "--"
}

#[derive(Debug, Clone, Default)]
struct Occurrences {
    values: BTreeMap<String, Vec<String>>,
}

impl Occurrences {
    fn record(&mut self, def: &OptionDef, values: Vec<String>) -> Option<()> {
        let entry = self.values.entry(def.name.clone());
        match entry {
            std::collections::btree_map::Entry::Occupied(mut occupied) => {
                if def.value != OptionValue::None && !def.repeated {
                    return None;
                }
                occupied.get_mut().extend(values);
            }
            std::collections::btree_map::Entry::Vacant(vacant) => {
                vacant.insert(values);
            }
        }
        Some(())
    }
}

/// The words a `many` option takes from `argv[from..]`.
fn many(argv: &[String], from: usize) -> Vec<String> {
    argv.iter()
        .skip(from)
        .take_while(|word| !(word.starts_with('-') && word.len() > 1))
        .cloned()
        .collect()
}

/// Read one option word at `argv[index]`, and return the index after it and its value.
fn read_option(
    table: &OptionTable,
    argv: &[String],
    index: usize,
    seen: &mut Occurrences,
    used: &mut Vec<usize>,
) -> Option<usize> {
    let word = &argv[index];
    if let Some(body) = word.strip_prefix("--") {
        let (name, attached) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        let at = *table.flags.get(&format!("--{name}"))?;
        let def = &table.defs[at];
        used.push(at);
        let (values, next) = match (def.value, attached) {
            (OptionValue::None, Some(_)) => return None,
            (OptionValue::None, None) => (Vec::new(), index + 1),
            (_, Some(value)) => (vec![value.to_string()], index + 1),
            (OptionValue::Required, None) => (vec![argv.get(index + 1)?.clone()], index + 2),
            (OptionValue::Optional, None) => (vec![String::new()], index + 1),
            (OptionValue::Many, None) => {
                let values = many(argv, index + 1);
                if values.is_empty() {
                    return None;
                }
                let next = index + 1 + values.len();
                (values, next)
            }
        };
        seen.record(def, values)?;
        return Some(next);
    }
    let cluster: Vec<char> = word.chars().skip(1).collect();
    for (position, character) in cluster.iter().enumerate() {
        let at = *table.flags.get(&format!("-{character}"))?;
        let def = &table.defs[at];
        used.push(at);
        if def.value == OptionValue::None {
            seen.record(def, Vec::new())?;
            continue;
        }
        let attached: String = cluster[position + 1..].iter().collect();
        if attached.starts_with('=') {
            return None;
        }
        let (values, next) = if !attached.is_empty() {
            (vec![attached], index + 1)
        } else {
            match def.value {
                OptionValue::Required => (vec![argv.get(index + 1)?.clone()], index + 2),
                OptionValue::Optional => (vec![String::new()], index + 1),
                _ => {
                    let values = many(argv, index + 1);
                    if values.is_empty() {
                        return None;
                    }
                    let next = index + 1 + values.len();
                    (values, next)
                }
            }
        };
        seen.record(def, values)?;
        return Some(next);
    }
    Some(index + 1)
}

/// Distribute operand words over the action's operand slots.
fn fill(
    action: &ActionGrammar,
    before: Vec<String>,
    after: Vec<String>,
    separated: bool,
) -> Option<Vec<Vec<String>>> {
    let slots = &action.operands;
    let mut filled = vec![Vec::new(); slots.len()];
    let groups: Vec<(std::ops::Range<usize>, Vec<String>)> = match action.separator {
        Separator::None => vec![(0..slots.len(), [before, after].concat())],
        Separator::Optional(_) if !separated => vec![(0..slots.len(), before)],
        Separator::Optional(at) | Separator::Required(at) => {
            vec![(0..at, before), (at..slots.len(), after)]
        }
    };
    for (range, words) in groups {
        let group = &slots[range.clone()];
        let minimum = group.iter().filter(|slot| slot.required).count();
        if words.len() < minimum {
            return None;
        }
        let mut extra = words.len() - minimum;
        let mut words = words.into_iter();
        for (offset, slot) in group.iter().enumerate() {
            let take = usize::from(slot.required)
                + if slot.variadic {
                    std::mem::take(&mut extra)
                } else if !slot.required && extra > 0 {
                    extra -= 1;
                    1
                } else {
                    0
                };
            filled[range.start + offset] = words.by_ref().take(take).collect();
        }
        if words.next().is_some() {
            return None;
        }
    }
    Some(filled)
}

fn apply(
    rules: &EffectRules,
    values: &[String],
    adds: &mut BTreeSet<String>,
    removes: &mut BTreeSet<String>,
) {
    let matches = |patterns: &[String]| {
        values
            .iter()
            .any(|value| patterns.iter().any(|pattern| like(pattern, value)))
    };
    if rules.like.is_empty() || matches(&rules.like) {
        adds.extend(rules.adds.iter().cloned());
        removes.extend(rules.removes.iter().cloned());
    }
    for rule in &rules.when {
        if matches(&rule.like) {
            adds.extend(rule.adds.iter().cloned());
            removes.extend(rule.removes.iter().cloned());
        }
    }
}

/// Cedar `like`: `*` matches any sequence, and `\*` is a literal `*`.
pub(crate) fn like(pattern: &str, value: &str) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Element {
        Any,
        Char(char),
    }
    let mut elements = Vec::new();
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&'*') => {
                characters.next();
                elements.push(Element::Char('*'));
            }
            '*' => elements.push(Element::Any),
            other => elements.push(Element::Char(other)),
        }
    }
    let value: Vec<char> = value.chars().collect();
    let mut reachable = vec![false; value.len() + 1];
    reachable[0] = true;
    for element in elements {
        let mut next = vec![false; value.len() + 1];
        match element {
            Element::Any => {
                let mut on = false;
                for (index, slot) in next.iter_mut().enumerate() {
                    on |= reachable[index];
                    *slot = on;
                }
            }
            Element::Char(expected) => {
                for index in 0..value.len() {
                    if reachable[index] && value[index] == expected {
                        next[index + 1] = true;
                    }
                }
            }
        }
        reachable = next;
    }
    reachable[value.len()]
}
