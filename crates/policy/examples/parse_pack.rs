//! Print the parse of command lines under one pack: `parse_pack <pack.toml> [-- argv...]`.
//!
//! With no argv after the pack, each stdin line is one command line, split on whitespace with
//! single and double quotes grouping words, and the program word first. `PARSE_PACK_SCHEMA=1`
//! prints the generated schema, and `PARSE_PACK_POLICY=<policy.dw>` validates a policy against it.

use std::io::BufRead as _;
use std::path::Path;

fn split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for character in line.chars() {
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (Some(open), _) if open == character => quote = None,
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: parse_pack <pack.toml> [-- <program> <args>...]");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(&path).expect("read the pack");
    let pack = match policy::Pack::parse(&text, Path::new(&path)) {
        Ok(pack) => pack,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let rest: Vec<String> = args.skip_while(|arg| arg == "--").collect();
    if !rest.is_empty() {
        println!("{}", pack.parse_command(&rest[1..]));
        return;
    }
    if let Some(policy) = std::env::var_os("PARSE_PACK_POLICY") {
        let source = policy::Policy {
            origin: policy.clone().into(),
            text: std::fs::read_to_string(&policy).expect("read the policy"),
        };
        match policy::PolicyEngine::validate_with_packs(
            &policy::Operator::unanchored(),
            &[source],
            std::slice::from_ref(&pack),
        ) {
            Ok(()) => println!("policy validates"),
            Err(error) => {
                println!("policy refused: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::var_os("PARSE_PACK_SCHEMA").is_some() {
        print!("{}", pack.schema());
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("read stdin");
        let words = split(&line);
        if words.is_empty() {
            continue;
        }
        println!("{}\t{}", line, pack.parse_command(&words[1..]));
    }
}
