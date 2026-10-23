# Tool packs

A tool pack teaches Box the commands of one program, such as `git`, `cargo`, or `aws`, so you can
write policy about what a command means instead of how it is spelled. Without a pack, a rule that
forbids force pushes has to list `-f`, `--force`, `-uf`, `--force-with-lease`, `+main`, and
`git -C repo push -f`. With the git pack, every one of those becomes the same action with the same
effect:

```dw
forbid (principal, action == Tool::git::Action::"push", resource)
when { context.input.effects.contains(Tool::git::Effect::"force_push") };
```

A pack is a TOML file. It holds data, never code. It names a program, the actions you can write
rules about, the effects those actions can have, and a grammar that says how to recognize each
action on a command line. This page is the reference for that file.

> Tool packs are a prototype on the `feature/policy-packs` branch. Only shell grammars exist today.

## How a pack fits in

You install a pack in `box.toml`, beside the `[tool]` table that already lets the agent run the
program:

```toml
tool_packs = ["packs/git.pack.toml"]

[tool.git]
command = ["git"]
```

When Box starts, it does three things with each pack:

1. **Checks the file.** A missing or unknown `format`, a missing or invalid `version`, any key it
   does not know, any effect that is used but not declared, and any usage line it cannot read stops
   Box from starting. Each error names the pack file, and the table when there is one.
2. **Generates a schema.** Every action becomes `Tool::<pack>::Action::"<action>"`, and every effect
   becomes `Tool::<pack>::Effect::"<effect>"`. From version 2 of a pack, the namespace is
   `Tool::<pack>::v<N>` (see [Versions](#versions)). Your policy is checked against that schema, so a rule
   that names an action or effect the pack does not have stops Box from starting.
3. **Binds the pack to a program.** Box finds the `[tool]` table whose program has the pack's
   `program` name, and remembers the real file it resolves to. If no `[tool]` runs that program,
   Box refuses to start, because the pack's rules would never apply.

When the agent runs a command, Box makes one decision about it. If the program is bound to a pack,
Box parses the command line with the pack's grammar and decides only the resulting
`Tool::<pack>::Action::"<action>"`. Every other program is decided by `shell:spawn`, as before. A
`shell:spawn` rule does not apply to a program a pack binds.

A pack action is an action like any other. It needs a permit, and with no permit it is denied. A
forbid still wins over a permit. `Tool::<pack>::Action::"*"` is a group that holds every action of
the pack, `unmodeled` included, so this one rule lets the agent run any git command:

```dw
permit (principal, action in Tool::git::Action::"*", resource);
```

A command line the grammar cannot read becomes `Tool::<pack>::Action::"unmodeled"`. So does the
bound file run under a name that no `[tool]` declares, such as a link to it. It is denied unless a
permit covers it. To allow only the commands the pack models, permit them by name and leave
`unmodeled` out:

```dw
permit (principal, action in [Tool::git::Action::"status", Tool::git::Action::"log"], resource);
```

## The file, annotated

Every key a pack can hold appears below. The comments say what each one does, and the sections
after it explain the parts that need more than a comment.

```toml
# ── Identity ────────────────────────────────────────────────────────────────

format  = 1         # Required. The version of the pack file language. Box reads 1.
version = 1         # Required. The major version of this pack's content. See "Versions".
pack    = "git"     # Required. The namespace: rules say Tool::git::... Letters, digits, '_'.
program = "git"     # Required. The program name the grammar covers. A name, never a path.

# ── Effects ─────────────────────────────────────────────────────────────────
# What a command can do, independent of which action does it. Each key becomes
# Tool::git::Effect::"<key>". The value is a description for people; Box ignores it.
# Every effect an action, option, or operand names must be declared here.

[effects]
force_push        = "Replaces commits on a remote ref"
delete_remote_ref = "Deletes a ref on the remote"
override_config   = "Overrides git configuration for this one command"
network           = "Contacts a remote repository"

# ── Global options ──────────────────────────────────────────────────────────
# Options that come before the command words, such as `git -C dir push`.
# An action accepts them only when its usage line says `[global options]`.

[global]
options.directory = { flag = ["-C"], value = "required" }
options.config    = { flag = ["-c"], value = "required", repeated = true, adds = ["override_config"] }
# `anywhere = true` also accepts the option after the command words, as `aws s3 ls --region x` does.
# It is allowed only here, in [global].

# ── Actions ─────────────────────────────────────────────────────────────────
# One [shell.<action>] table per action. The table name becomes
# Tool::git::Action::"<action>". "unmodeled" is reserved.

[shell.push]
# Required. The shape of the command line. See "The usage line" below.
usage = "git [global options] push [options] [<remote> [<refspec>...]]"

# Optional. Other command words for the same action, such as `cargo t` for `cargo test`.
# aliases = ["p"]

# Optional. Effects every push has, whatever its options are.
adds = ["network"]

# Options this action accepts after its command words. The key is the field name that
# rules read as context.input.<key>.
options.force            = { flag = ["-f", "--force"], adds = ["force_push"] }
options.force_with_lease = { flag = ["--force-with-lease"], value = "optional", adds = ["force_push"] }
options.dry_run          = { flag = ["-n", "--dry-run"], removes = ["force_push"] }
options.push_option      = { flag = ["-o", "--push-option"], value = "required", repeated = true }

# Operands are named by the <placeholders> in the usage line. You only need an entry
# for an operand that carries effects.
operands.refspec = { like = "+*", adds = ["force_push"],
                     when = [ { like = ":*", adds = ["delete_remote_ref"] } ] }
```

### An option entry

| Key | Default | Meaning |
|---|---|---|
| `flag` | required | One spelling or a list. Each is `-x` or `--long-name`, never with `=`. |
| `value` | `"none"` | `"none"`: a switch. `"required"`: takes a value, as `-C dir`, `-Cdir`, or `--opt=v`. `"optional"`: takes a value only when it is attached, as `--force-with-lease` or `--force-with-lease=main`. `"many"`: takes every word up to the next option. |
| `repeated` | `false` | The option may appear more than once. Without it, a second use makes the line unmodeled. |
| `adds` | `[]` | Effects the option adds. |
| `removes` | `[]` | Effects the option takes away. |
| `like` | none | A pattern on the option's value. With it, `adds` and `removes` apply only when a value matches. |
| `when` | `[]` | More pattern rules, each `{ like, adds, removes }`, checked one by one. |
| `anywhere` | `false` | `[global]` only. The option may also come after the command words. |

A switch (`value = "none"`) has no value, so it cannot carry `like`, `when`, or `repeated`.

### An operand entry

| Key | Default | Meaning |
|---|---|---|
| `like`, `adds`, `removes`, `when` | | The same as on an option. The pattern is checked against each word the operand took. |
| `rest` | `false` | From the first operand word on, every word belongs to this operand, even one that looks like an option. `cargo run x --release` passes `--release` to the program. It must be the only operand. |

### Patterns

`like` uses Cedar's `like` rules: `*` matches any run of characters, and `\*` is a literal `*`.
Nothing else is special. A pattern can be one string or a list, and a list matches when any of its
patterns matches.

### How effects add up

The effects of one command are the action's own `adds`, plus every `adds` from the options and
operands present on the line, minus every `removes`. **A `removes` always wins**, wherever it sits
on the line, so `git push -f --dry-run` has no `force_push` effect.

## Versions

A pack carries two version numbers, and they answer different questions.

`format` is the version of the pack file language: the keys on this page and what they mean. Box
owns it. Every pack today says `format = 1`, and Box refuses to start with a pack whose `format` is
missing or is a number it does not read.

`version` is the major version of the pack's own content: its actions, effects, and grammar. The
pack's author owns it. It is a whole number, 1 or more.

The version is part of the namespace, the way Go puts a major version in a module path. Version 1
has no suffix. Version 2 and later add `v<N>`:

| `version` | A rule names |
|---|---|
| `1` | `Tool::git::Action::"push"`, `Tool::git::Effect::"force_push"` |
| `2` | `Tool::git::v2::Action::"push"`, `Tool::git::v2::Effect::"force_push"` |

So a policy is written against one major version. If you install version 2 of a pack and your
policy still says `Tool::git::Action::"push"`, the policy names an action the schema does not have,
and Box refuses to start. You never get a policy that silently means something else. `box.toml` does
not change: `tool_packs` still lists paths.

An author bumps `version` when a command line the old version recognized now gives a different
action or different effects. A rule written against the old meaning could then allow what it
meant to deny. Recognizing a line that used to be `unmodeled` is not a bump, as long as its
effects are right, because no rule could have allowed that line by name before.

## The usage line

The usage line is not just documentation. Box reads it to learn the command words, where options
go, and which operands exist. It uses the bracket style of a `--help` page:

| Token | Meaning |
|---|---|
| `git` | The first word must be the pack's `program`. |
| `push`, `stash list` | Command words: lowercase letters, digits, and `-`. They must be required and next to each other. |
| `[global options]` | The `[global]` options may come here. It must come before the command words. |
| `[options]` | This action's `options.*` may come here. It must come after the command words, and a table that declares options must include it. |
| `<name>` | A required operand. The rule reads it as `context.input.name`. |
| `[<name>]` | An optional operand. |
| `<name>...` | An operand that takes one or more words. |
| `--` or `[--]` | A required or optional end-of-options marker. Operands after it fill the operands after it. At most one per line. |
| `[+<toolchain>]` | A rustup-style `+stable` selector. It must come straight after the program. |
| `[--quiet]`, `[-C <dir>]`, `[--format=<fmt>]`, `[--opt[=<v>]]` | An option written inline. It becomes a field with no effects. A bare short flag such as `[-q]` needs an `options` entry, because it has no name to use as a field. |

`|`, `(`, `)`, `{`, and `}` are not supported. Write one action per alternative instead.

When several actions could match, the one with the most command words wins: an action spelled
`stash list` wins over one spelled `stash` for `git stash list`.

## What a rule can read

Each action gets its own input type, generated from its table:

| Field | Type | From |
|---|---|---|
| `program` | `String` | The name the agent called the program by |
| `program_path` | `String` | The real file that ran |
| `cwd` | `String` | The working directory |
| `args` | `Set<String>` | Every word after the program |
| `effects` | `Set<Effect>` | The effects, as worked out above |
| `credential_reads` | `Set<String>`, optional | Each credential store the matching `[tool.<name>.filesystem]` lists name, such as `~/.aws`, as on `shell:spawn` |
| a switch | `Bool` | `true` when present |
| an option with `value = "many"` or `repeated = true` | `Set<String>` | Every value given |
| any other option | `String`, optional | The value given |
| a single operand | `String`, optional | The word given |
| a `...` or `rest` operand | `Set<String>` | Every word given |
| `[+<toolchain>]` | `String`, optional | The selector, without the `+` |

An optional field is absent when the command does not use it. Box checks policy strictly, so a rule
must test `has` before it reads one:

```dw
forbid (principal, action == Tool::git::Action::"push", resource)
when { context.input has remote && context.input.remote == "upstream" };
```

Every pack also generates a group, `Tool::<pack>::Action::"*"`, that holds all of its actions. Use
it for a rule about an effect that several actions share:

```dw
forbid (principal, action in Tool::cargo::Action::"*", resource)
when { context.input.effects.contains(Tool::cargo::Effect::"update_lockfile") };
```

After each action runs, Box records `Tool::<pack>::Action::"<action>"::response` with the exit
status in `output.status`, so a temporal rule can require that an earlier command succeeded. A
program a pack binds is recorded in history under its pack action only, and not as `shell:spawn`:

```dw
forbid (principal, action == Tool::git::Action::"push", resource)
unless temporal { formerly within 1h (Tool::cargo::Action::"test"::response{ output.status: 0 }) };
```

## When a line is unmodeled

The grammar never guesses. Any of these makes the whole command `unmodeled`:

- The command words match no action, such as a git alias like `git ci`.
- An option the action does not declare, including a newer flag the pack does not know yet.
- A global option after the command words, unless it is `anywhere`.
- A `[global]` option on an action whose usage line has no `[global options]`.
- A non-repeated option given twice, or a switch given a value (`--force=yes`).
- Too few or too many operands.
- `--` before the command words.

This is why an out-of-date pack fails toward `unmodeled`. It does not report a dangerous command as a
safe one, and `unmodeled` runs only when a permit covers it.

## Check a pack by hand

The `parse_pack` example prints what a pack makes of each command line:

```sh
printf '%s\n' 'git -C app push -uf origin main' 'git push -f --dry-run' 'git ci -m x' \
  | cargo run -q -p strands-box-policy --all-features --example parse_pack -- examples/packs/git.pack.toml
```

```text
git -C app push -uf origin main   push   effects: force_push, network   (directory="app", force, set_upstream)
git push -f --dry-run             push   effects: network               (force, dry_run)
git ci -m x                       unmodeled
```

The real output is one JSON object per line, with every field. Set `PARSE_PACK_SCHEMA=1` to print
the generated schema, or `PARSE_PACK_POLICY=policy.dw` to check a policy against the pack.

The packs in [`examples/packs/`](../../examples/packs/) for `git`, `cargo`, and `aws` are complete
examples, and each has a corpus in `examples/packs/corpus/` that lists command lines with the action
and effects the pack should give them.

## What a pack cannot see

A shell grammar reads only the command line. It does not see environment variables, configuration
files, git aliases, hooks, or programs that the tool starts by itself. A pack makes the common
spellings of a command match one rule. It is not a guarantee against an agent that is trying to get
around that rule.
