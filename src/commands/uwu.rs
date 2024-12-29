use crate::Context;

use anyhow::Result;
use crate::common::uwuify;

#[poise::command(slash_command, prefix_command)]
pub async fn uwu(
    ctx: Context<'_>,
    #[description = "The text to uwuify"] message: Option<String>,
) -> Result<()> {
    let reply = match message {
        Some(message) => uwuify(&message),
        None => "You must".parse()?,
    };
    ctx.say(reply).await?;
    Ok(())
}