use crate::prelude::*;

#[command("false")]
async fn cmd_false(_os: &Mediated, _args: &[String]) -> CommandResult {
    Ok(1)
}
