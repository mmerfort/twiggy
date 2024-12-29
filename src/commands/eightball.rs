use crate::Context;

use anyhow::Result;
use rand::seq::SliceRandom;

const REPLIES: &[&str] = &[
    "It is certain.",
    "It is decidedly so.",
    "Without a doubt.",
    "Yes definitely.",
    "You may rely on it.",
    "As I see it, yes.",
    "Most likely.",
    "Outlook good.",
    "Yes.",
    "Signs point to yes.",
    "Reply hazy, try again.",
    "Ask again later.",
    "Better not tell you now.",
    "Cannot predict now.",
    "Concentrate and ask again.",
    "Don't count on it.",
    "My reply is no.",
    "My sources say no.",
    "Outlook not so good.",
    "Very doubtful. ",
];

const FALK_REPLIES: &[&str] = &[
    "ofc",
    "dis so",
    "undoubtederably",
    "yassss deffff",
    "rely on it, queen **uwu**",
    "imo? ya",
    "prolly",
    "loikely",
    "ngl it b lukken gud (like u **uwu**)",
    "yassss",
    "signs b pointerin 2 de yass",
    "reply hazy... try again when I's b dun wif ur mum",
    "ask again l8r 'bater",
    "ngl I's shudnt b tellerin u now",
    "unpredicterable",
    "concentrate n ask again wif more respect, loser **uwu**",
    "dun b counterin on it :MingLow:",
    "no. hecc u",
    "ma source code says no",
    "outlook not so good... like microsoft's outlook (gottem)",
    "¡ayy! muchos doubtidos, famigo",
    "yasss o nah",
];

/// Magic 8 Ball in Rust
#[poise::command(slash_command, prefix_command)]
pub async fn eightball(
    ctx: Context<'_>,
    #[description = "The question you want to ask the 8 Ball"] message: Option<String>,
) -> Result<()> {
    let fortune = {
        // make sure rng goes out of scope before you call await
        let mut rng = rand::thread_rng();
        REPLIES
            .choose(&mut rng)
            .expect("Expected to have at least 1 choice")
    };
    let reply = match message {
        Some(message) => format!("{message} - {fortune}"),
        None => fortune.to_string(),
    };
    ctx.say(reply).await?;

    Ok(())
}

/// majicc 8bol
#[poise::command(slash_command, prefix_command)]
pub async fn fball(
    ctx: Context<'_>,
    #[description = "majicc 8bol\" n \"d Q u wan2b askerin d 8bol"] message: Option<String>
) -> Result<()> {
    let fortune = {
        let mut rng = rand::thread_rng();
        FALK_REPLIES
            .choose(&mut rng)
            .expect("Expected to have at least 1 choice")
    };
    let reply = match message {
        Some(message) => format!("{message} - {fortune}"),
        None => fortune.to_string(),
    };
    ctx.say(reply).await?;
    
    Ok(())
}