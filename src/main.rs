mod commands;
mod common;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::OnceLock;

use anyhow::Result;
use config::Config;
use commands::*;
use common::{response, text_message};
use lru::LruCache;
use poise::serenity_prelude::{self as serenity, CreateCommand, FullEvent};
use poise::Command;
use tokio::sync::{Mutex, RwLock};

pub struct Data {
    database: sqlx::SqlitePool,
    rpg_summary_cache: Mutex<LruCache<u64, String>>,
    simple_commands: RwLock<SimpleCommands>,
    configuration: HashMap<String, String>,
}
pub type Context<'a> = poise::Context<'a, Data, anyhow::Error>;
pub type Error = anyhow::Error;

pub const SUB_ROLE_ID: u64 = 930791790490030100;
pub static DEFAULT_COMMANDS: OnceLock<Vec<String>> = OnceLock::new();

#[tokio::main]
async fn main() {
    let settings = Config::builder()
        .set_default("mixuPrefix", "miku").unwrap()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("APP"))
        .build()
        .unwrap();
    let configuration = settings.try_deserialize::<HashMap<String, String>>().unwrap();
    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    if std::env::var("WOLFRAM_APP_ID").is_err() {
        eprintln!("[WARNING] The /ask command does not work without a Wolfram Alpha App ID, set WOLFRAM_APP_ID as an env variable.");
    }

    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename("database.sqlite")
                .create_if_missing(true),
        )
        .await
        .expect("Expected to be able to connect to the database");

    let commands: Vec<Command<Data, Error>> = vec![
        poll(),
        ask(),
        bestmixu(),
        color(),
        commands(),
        dino(),
        duel(),
        eightball(),
        fball(),
        mikustare(),
        mixu(),
        quote(),
        quwuote(),
        rpg(),
        rps(),
        sudoku(),
        uncolor(),
        uwu(),
    ];

    // Initialize default commands
    DEFAULT_COMMANDS.get_or_init(|| commands.iter().map(|c| c.name.clone()).collect::<Vec<_>>());

    // Initialize mixu score
    set_initial_best_mixu_score(&database)
        .await
        .expect("Unable to set best mixu score");

    let options = poise::FrameworkOptions {
        commands,
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(String::from(">")),
            mention_as_prefix: false,
            ..Default::default()
        },
        event_handler: |ctx, event, framework, user_data| {
            Box::pin(event_event_handler(ctx, event, framework, user_data))
        },
        on_error: |err| Box::pin(on_error(err)),
        ..Default::default()
    };

    let user_data = Data {
        database,
        rpg_summary_cache: Mutex::new(LruCache::new(NonZeroUsize::new(10).unwrap())),
        simple_commands: RwLock::default(),
        configuration,
    };
    let framework = poise::Framework::builder()
        .options(options)
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                // poise::builtins::register_globally(ctx, Vec::new()).await?;
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(user_data)
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    match error {
        poise::FrameworkError::Command { error, .. } => {
            eprintln!("Command error: {}", error);
        }
        _ => {
            if let Err(e) = poise::builtins::on_error(error).await {
                eprintln!("Error while trying to handle poise error: {e}")
            }
        }
    }
}

async fn event_event_handler<'a>(
    ctx: &'a serenity::Context,
    event: &'a serenity::FullEvent,
    _framework: poise::FrameworkContext<'a, Data, Error>,
    user_data: &Data,
) -> Result<(), Error> {
    match event {
        FullEvent::Ready { data_about_bot } => {
            println!("{} is connected!", data_about_bot.user.name);

            let commands_map = fetch_guild_commands(user_data)
                .await
                .expect("Could not fetch simple guild commands");
            register_guild_commands(ctx, &commands_map)
                .await
                .expect("Could not register simple guild commands");
            let mut data_commands = user_data.simple_commands.write().await;
            *data_commands = commands_map;
            // NOTE: Drop here because tokio::select! keeps the lock otherwise
            drop(data_commands);

            tokio::select! {
                _ = setup_rpg_summary(ctx, user_data) => {}
                _ = setup_dino_collector(ctx, user_data) => {}
            }
        }
        FullEvent::InteractionCreate { interaction } => {
            let interaction = interaction.clone();
            let Some(command) = interaction.command() else {
                return Ok(());
            };

            let map = user_data.simple_commands.read().await;
            let Some(guild_id) = command.guild_id else {
                return Ok(());
            };
            let Some(guild_commands) = map.get(&(guild_id.get() as i64)) else {
                return Ok(());
            };

            if let Some(text) = guild_commands.get(&command.data.name) {
                command
                    .create_response(ctx, response(text_message(text)))
                    .await?;
            };
        }
        _ => {}
    }

    Ok(())
}

#[derive(Debug)]
struct GuildCommand {
    guild_id: i64,
    name: String,
    content: String,
}

async fn fetch_guild_commands(user_data: &Data) -> Result<HashMap<i64, HashMap<String, String>>> {
    let guild_commands = sqlx::query_as!(
        GuildCommand,
        "SELECT guild_id, name, content FROM SimpleCommands"
    )
    .fetch_all(&user_data.database)
    .await?;

    let mut commands_map: HashMap<i64, HashMap<String, String>> = HashMap::new();
    for command in guild_commands {
        let entry = commands_map.entry(command.guild_id).or_default();
        entry.insert(command.name, command.content);
    }

    Ok(commands_map)
}

async fn register_guild_commands(
    ctx: &serenity::Context,
    commands_map: &SimpleCommands,
) -> Result<()> {
    for id in ctx.cache.guilds() {
        let Some(names) = commands_map.get(&(id.get() as i64)) else {
            // Reset commands if there aren't any for this guild
            id.set_commands(&ctx.http, Vec::new()).await?;
            continue;
        };

        let commands = names
            .iter()
            .map(|(name, _content)| CreateCommand::new(name).description("A simple text command"))
            .collect::<Vec<_>>();

        id.set_commands(ctx, commands).await?;
    }

    Ok(())
}
