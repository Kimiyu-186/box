use crate::prelude::*;

#[command("true")]
async fn cmd_true(_os: &Mediated, _args: &[String]) -> CommandResult {
    Ok(0)
}
