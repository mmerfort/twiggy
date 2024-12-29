use crate::common::{
    avatar_url, bail_reply, colour, ephemeral_text_message, name, reply_with_buttons, response,
    text_message, update_response,
};
use crate::Context as DiscordContext;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use poise::serenity_prelude::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedAuthor, Member, User,
    UserId,
};
use poise::{CreateReply, ReplyHandle};
use rand::Rng;
use serenity::all::{ComponentInteraction, ComponentInteractionCollector, MessageId};
use sqlx::{Connection, SqliteExecutor, Transaction};
use std::cmp::Ordering;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;

// TODO: this should be replaced with a const chrono::Duration when that gets stabilized
const LOSS_COOLDOWN: i64 = 60;
const DEAD_DUEL_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const TIMEOUT_DURATION: Duration = Duration::from_secs(10 * 60);

static IN_PROGRESS: AtomicBool = AtomicBool::new(false);

#[poise::command(slash_command, guild_only, subcommands("challenge", "stats"))]
pub async fn duel(_: DiscordContext<'_>) -> Result<()> {
    // Do nothing because this is just for subcommands
    Ok(())
}

/// Challenge the chat to a duel
#[poise::command(slash_command, guild_only)]
pub async fn challenge(ctx: DiscordContext<'_>) -> Result<()> {
    let challenger = DuelUser::from(ctx, ctx.author()).await;

    if IN_PROGRESS.load(AtomicOrdering::Acquire) {
        return bail_reply(ctx, "A duel is already in progress").await;
    }

    if let Err(e) = challenger.ensure_outside_cooldown(ctx).await {
        return bail_reply(ctx, e.to_string()).await;
    }

    let reply_content = format!("{challenger} is looking for a duel, press the button to accept.");
    let reply_handle = ctx
        .send(reply_with_buttons(
            reply_content,
            vec![create_accept_button()],
        ))
        .await?;

    // Make sure the in_progress status gets updated even on failure
    IN_PROGRESS.store(true, AtomicOrdering::Release);
    if let Err(e) = run_duel(ctx, challenger, reply_handle).await {
        eprintln!("Failed to run duel to completiton: {e:?}");
    }
    IN_PROGRESS.store(false, AtomicOrdering::Release);

    Ok(())
}

async fn run_duel(
    ctx: DiscordContext<'_>,
    challenger: DuelUser,
    reply_handle: ReplyHandle<'_>,
) -> Result<()> {
    let message = reply_handle.message().await?;
    let opponent = find_opponent(ctx, message.id, challenger.id.get()).await;

    let Some((interaction, accepter)) = opponent else {
        let duel_timeout_msg = format!("{challenger} failed to find someone to duel.");

        reply_handle
            .edit(ctx, reply_with_buttons(duel_timeout_msg, Vec::new()))
            .await?;

        return Ok(());
    };

    let (challenger_score, accepter_score) = pick_scores();

    let mut conn = ctx.data().database.acquire().await?;
    let mut transaction = conn.begin().await?;

    let winner_text = match challenger_score.cmp(&accepter_score) {
        Ordering::Greater => {
            let (winner_id, loser_id) = (&challenger.string_id, &accepter.string_id);
            update_users_win_loss(&mut transaction, winner_id, loser_id).await?;
            update_last_loss(&mut transaction, &accepter.string_id).await?;

            format!("{challenger} has won!")
        }
        Ordering::Less => {
            let (winner_id, loser_id) = (&accepter.string_id, &challenger.string_id);
            update_users_win_loss(&mut transaction, winner_id, loser_id).await?;
            update_last_loss(&mut transaction, loser_id).await?;

            format!("{accepter} has won!")
        }
        Ordering::Equal => {
            update_users_drawn(&mut transaction, &challenger.string_id, &accepter.string_id)
                .await?;

            let timeout_end_time = Utc::now() + chrono::Duration::from_std(TIMEOUT_DURATION)?;
            let challenger_member = ctx.author_member().await.map(|m| m.into_owned());
            timeout_user(ctx, challenger_member, timeout_end_time).await;
            timeout_user(ctx, interaction.member.clone(), timeout_end_time).await;

            "It's a draw! Now go sit in a corner for 10 mintues and think about your actions..."
                .into()
        }
    };

    let final_message = format!("{accepter} has rolled a {accepter_score} and {challenger} has rolled a {challenger_score}. {winner_text}");
    let update_resp = update_response(text_message(final_message).components(Vec::new()));
    interaction.create_response(ctx, update_resp).await?;

    transaction.commit().await?;

    Ok(())
}

async fn find_opponent(
    ctx: DiscordContext<'_>,
    message_id: MessageId,
    challenger_id: u64,
) -> Option<(ComponentInteraction, DuelUser)> {
    while let Some(interaction) = ComponentInteractionCollector::new(ctx)
        .message_id(message_id)
        .filter(move |f| f.data.custom_id == "duel-btn")
        .timeout(DEAD_DUEL_COOLDOWN)
        .await
    {
        // NOTE: responding with an ephemeral message does not trigger the
        // `iteraction failed` error but I'd like to find a way to just ignore
        // the click entirely with no response.
        if interaction.user.id == challenger_id {
            let resp = response(ephemeral_text_message("You cannot join your own duel."));
            interaction.create_response(ctx, resp).await.ok()?;
            continue;
        }

        if !IN_PROGRESS.load(AtomicOrdering::Acquire) {
            let resp = response(ephemeral_text_message(
                "Someone beat you to the challenge already",
            ));
            interaction.create_response(ctx, resp).await.ok()?;
            continue;
        }

        let accepter = DuelUser::from(ctx, &interaction.user).await;
        if let Err(e) = accepter.ensure_outside_cooldown(ctx).await {
            let resp = response(ephemeral_text_message(e.to_string()));
            interaction.create_response(ctx, resp).await.ok()?;
            continue;
        }

        return Some((interaction, accepter));
    }

    None
}

/// Display your duel statistics
#[poise::command(slash_command)]
pub async fn stats(
    ctx: DiscordContext<'_>,
    #[description = "Silent"] silent: Option<bool>,
) -> Result<()> {
    // let silent_output = silent.unwrap_or_else(|| true);
    let user = ctx.author();
    let conn = &mut ctx.data().database.acquire().await?;

    let Some(stats) = get_duel_stats(conn, user.id.to_string()).await? else {
        return bail_reply(ctx, "You have never dueled before.").await;
    };

    let name = name(&ctx, user).await;
    let colour = colour(&ctx).await.unwrap_or_else(|| 0x77618F.into());
    let embed = CreateEmbed::default()
        .colour(colour)
        .description(format!(
            "{}\n{}\n{}",
            stats.current_streak(),
            stats.best_streak(),
            stats.worst_streak()
        ))
        .author(
            CreateEmbedAuthor::new(format!(
                "{name}'s scoresheet: {}-{}-{}",
                stats.wins, stats.losses, stats.draws
            ))
            .icon_url(avatar_url(user)),
        );

    if silent.unwrap_or(true) {
        ctx.defer_ephemeral().await?;
    }
    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

async fn get_last_loss(executor: impl SqliteExecutor<'_>, user_id: &str) -> Result<NaiveDateTime> {
    // Insert a new User so that DuelStats always has a user to reference when
    // we set the wins/losses/draws after the duel
    let row = sqlx::query!(
        r#"
        INSERT OR IGNORE INTO User (id) VALUES (?);
        SELECT last_loss From User WHERE id = ?
        "#,
        user_id,
        user_id
    )
    .fetch_one(executor)
    .await
    .with_context(|| format!("Failed to get {user_id}'s last loss"))?;

    Ok(row.last_loss)
}

async fn update_last_loss(executor: impl SqliteExecutor<'_>, user_id: &str) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO User (id, last_loss) VALUES (?, datetime('now'))
        ON CONFLICT(id) DO UPDATE SET last_loss = datetime('now')"#,
        user_id
    )
    .execute(executor)
    .await
    .with_context(|| format!("Failed to update {user_id}'s last loss"))?;

    Ok(())
}

async fn update_users_win_loss(
    executor: &mut Transaction<'_, sqlx::Sqlite>,
    winner_id: &str,
    loser_id: &str,
) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO DuelStats (user_id, wins, win_streak, win_streak_max)
        VALUES (?, 1, 1, 1)
        ON CONFLICT(user_id) DO UPDATE SET
            wins = wins + 1,
            win_streak = win_streak + 1,
            win_streak_max = MAX(win_streak_max, win_streak + 1),
            loss_streak = 0;

        INSERT INTO DuelStats (user_id, losses, loss_streak, loss_streak_max)
        VALUES (?, 1, 1, 1)
        ON CONFLICT(user_id) DO UPDATE SET
            losses = losses + 1,
            loss_streak = loss_streak + 1,
            loss_streak_max = MAX(loss_streak_max, loss_streak + 1),
            win_streak = 0;"#,
        winner_id,
        loser_id
    )
    .execute(&mut *executor)
    .await
    .with_context(|| format!("Failed to update {winner_id} and/or {loser_id}'s wins/losses"))?;

    Ok(())
}

async fn update_users_drawn(
    executor: &mut Transaction<'_, sqlx::Sqlite>,
    challenger_id: &str,
    accepter_id: &str,
) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO DuelStats (user_id, draws) VALUES (?, 1), (?, 1)
        ON CONFLICT(user_id)
        DO UPDATE SET draws = draws + 1, win_streak = 0, loss_streak = 0;"#,
        challenger_id,
        accepter_id
    )
    .execute(&mut *executor)
    .await
    .with_context(|| format!("Failed to update {challenger_id} and {accepter_id}'s draws"))?;

    Ok(())
}

struct DuelStats {
    #[allow(dead_code)]
    user_id: String,
    losses: i64,
    wins: i64,
    draws: i64,
    win_streak: i64,
    loss_streak: i64,
    win_streak_max: i64,
    loss_streak_max: i64,
}

impl DuelStats {
    fn current_streak(&self) -> String {
        match (self.win_streak, self.loss_streak, self.draws) {
            (0, 0, 0) => "You have never dueled before".to_string(),
            (0, 0, _) => "Your last duel was a draw".to_string(),
            (0, _, _) => format!("Current streak **{} losses**", self.loss_streak),
            (_, 0, _) => format!("Current streak **{} wins**", self.win_streak),
            _ => unreachable!(),
        }
    }

    fn best_streak(&self) -> String {
        format!("Best streak: **{} wins**", self.win_streak_max)
    }

    fn worst_streak(&self) -> String {
        format!("Worst streak: **{} losses**", self.loss_streak_max)
    }
}

async fn get_duel_stats(
    executor: impl SqliteExecutor<'_>,
    user_id: String,
) -> Result<Option<DuelStats>> {
    let stats = sqlx::query_as!(
        DuelStats,
        r#"SELECT * FROM DuelStats WHERE user_id = ?"#,
        user_id
    )
    .fetch_optional(executor)
    .await
    .with_context(|| format!("Failed to get {user_id}'s duel stats"))?;

    Ok(stats)
}

async fn timeout_user(ctx: DiscordContext<'_>, member: Option<Member>, until: DateTime<Utc>) {
    let Some(mut member) = member else {
        return;
    };

    if let Err(e) = member
        .disable_communication_until_datetime(ctx, until.into())
        .await
    {
        eprintln!("Failed to timeout {}, reason: {e:?}", member.user.name);
    }
}

fn create_accept_button() -> CreateActionRow {
    let btn = CreateButton::new("duel-btn")
        .emoji('🎲')
        .label("Accept Duel".to_string())
        .style(ButtonStyle::Primary);

    CreateActionRow::Buttons(vec![btn])
}

struct DuelUser {
    id: UserId,
    string_id: String,
    name: String,
}

impl DuelUser {
    async fn from(ctx: DiscordContext<'_>, user: &User) -> Self {
        let id = user.id;

        Self {
            id,
            string_id: id.to_string(),
            name: name(&ctx, user).await,
        }
    }

    async fn ensure_outside_cooldown(&self, ctx: DiscordContext<'_>) -> Result<()> {
        let last_loss = match get_last_loss(&ctx.data().database, &self.string_id).await {
            Ok(last_loss) => last_loss,
            Err(e) => {
                eprintln!("Could not get {self}'s last loss: {e:?}");
                bail!("Couldn't get your last loss, no duel for you! :<");
            }
        };

        let now = Utc::now().naive_utc();

        let loss_cooldown_duration = chrono::Duration::minutes(LOSS_COOLDOWN);
        if last_loss + loss_cooldown_duration > now {
            let time_until_duel = (last_loss + loss_cooldown_duration).and_utc().timestamp();
            bail!("{self} you have recently lost a duel. Please try again <t:{time_until_duel}:R>.")
        }

        Ok(())
    }
}

impl Display for DuelUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn pick_scores() -> (usize, usize) {
    let mut rng = rand::thread_rng();
    (rng.gen_range(0..=100), rng.gen_range(0..=100))
}
