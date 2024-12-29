use std::sync::atomic::{AtomicI64, Ordering};

use crate::{Context, Result};

use poise::serenity_prelude::{Emoji, Mention, UserId};
use rand::{seq::SliceRandom, thread_rng};
use serenity::futures::TryFutureExt;
use sqlx::SqlitePool;
use tokio::sync::OnceCell;

const MIXU_BANNER: &str =
    ":regional_indicator_m::regional_indicator_i::regional_indicator_x::regional_indicator_u:";
const MIKU_BANNER: &str =
    ":regional_indicator_m::regional_indicator_i::regional_indicator_k::regional_indicator_u:";
const MIKU_POSITIONS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

static MIXU_PIECES: OnceCell<Vec<Emoji>> = OnceCell::const_new();
static BEST_MIXU_SCORE: AtomicI64 = AtomicI64::new(0);

pub async fn set_initial_best_mixu_score(db: &SqlitePool) -> Result<()> {
    let row = sqlx::query!("SELECT score FROM BestMixu ORDER BY rowid DESC LIMIT 1")
        .fetch_optional(db)
        .await?;

    let score = row.map(|r| r.score).unwrap_or_default();
    BEST_MIXU_SCORE.store(score, Ordering::Relaxed);

    Ok(())
}

/// Generate a random mixu
#[poise::command(guild_only, slash_command, prefix_command)]
pub async fn mixu(ctx: Context<'_>) -> Result<()> {
    let mut positions = MIKU_POSITIONS;
    positions.shuffle(&mut thread_rng());

    let pieces = MIXU_PIECES
        .get_or_try_init(|| retrieve_mixu_emojis(ctx))
        .await?;
    let mixu = stringify_mixu(pieces, &positions, MIXU_BANNER);
    let score = count_points(&positions);

    tokio::try_join!(
        update_max_score(ctx, score, &positions),
        ctx.say(mixu).map_err(anyhow::Error::msg)
    )?;

    Ok(())
}

/// Have Miku stare into your soul
#[poise::command(guild_only, slash_command, prefix_command)]
pub async fn mikustare(ctx: Context<'_>) -> Result<()> {
    let pieces = MIXU_PIECES
        .get_or_try_init(|| retrieve_mixu_emojis(ctx))
        .await?;

    let miku = stringify_mixu(pieces, &MIKU_POSITIONS, MIKU_BANNER);
    ctx.say(miku).await?;

    Ok(())
}

/// See who sits on top of the mixu leaderboard
#[poise::command(guild_only, slash_command, prefix_command)]
pub async fn bestmixu(ctx: Context<'_>) -> Result<()> {
    let record = sqlx::query!("SELECT user_id, tiles FROM BestMixu ORDER BY rowid DESC LIMIT 1")
        .fetch_optional(&ctx.data().database)
        .await?;

    let Some((user_id, tiles)) = record.map(|r| (r.user_id, r.tiles)) else {
        ctx.say("The best Mixu is yet to come").await?;
        return Ok(());
    };

    let tiles = tiles
        .split(',')
        .map(str::parse)
        .collect::<Result<Vec<usize>, _>>()?;

    let pieces = MIXU_PIECES
        .get_or_try_init(|| retrieve_mixu_emojis(ctx))
        .await?;
    let message = stringify_mixu(pieces, &tiles, MIXU_BANNER);

    ctx.say(format!(
        "Best mixu by {}",
        Mention::User(UserId::new(user_id as u64))
    ))
    .await?;
    ctx.say(message).await?;

    Ok(())
}

fn stringify_mixu(pieces: &[Emoji], positions: &[usize], banner: &str) -> String {
    let mut output = String::with_capacity(128);
    output.push_str(banner);

    for i in 0..4 {
        output.push('\n');
        for j in 0..4 {
            let index = i * 4 + j;
            let position = positions[index];
            output.push_str(&pieces[position].to_string());
        }
    }

    output
}

async fn retrieve_mixu_emojis(ctx: Context<'_>) -> Result<Vec<Emoji>> {
    let emote_prefix = match ctx.data().configuration.get("mixuprefix") {
        Some(prefix) => prefix,
        None => "mixu",
    };
    let guild = ctx
        .guild_id()
        .expect("Expected Mixu commands to be guild only");
    let emojis = guild.emojis(ctx).await?;

    let mut miku_emoji_ids = Vec::with_capacity(16);
    for i in 1..=16 {
        let piece = emojis
            .iter()
            .find(|emoji| emoji.name == format!("{emote_prefix}{i}"))
            .ok_or_else(|| anyhow::anyhow!("Could not find miku piece {emote_prefix}{i}"))?;
        miku_emoji_ids.push(piece.clone());
    }

    Ok(miku_emoji_ids)
}

fn count_points(tiles: &[usize]) -> i64 {
    let mut count = 0;
    for row in 0..4 {
        for col in 0..4 {
            let index = row * 4 + col;
            let tile = tiles[index];

            if tile == index {
                count += 1;
            }

            // Not at the 4th col
            // There is no neighbor to the right of the tile in the 4th col
            // Next tile is its neighbor
            if col != 3 && tile % 4 != 3 && tiles[index + 1] == tile + 1 {
                count += 1;
            }

            // Not at last row
            // Tile under is its neighbor
            if row != 3 && tiles[index + 4] == tile + 4 {
                count += 1;
            }
        }
    }

    count as i64
}

async fn update_max_score(ctx: Context<'_>, score: i64, tiles: &[usize]) -> Result<()> {
    // NOTE: fetch_max returns the previous value stored
    if BEST_MIXU_SCORE.fetch_max(score, Ordering::SeqCst) >= score {
        return Ok(());
    }

    let tiles = tiles
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");

    let user_id = ctx.author().id.get() as i64;
    sqlx::query!(
        "INSERT INTO BestMixu (user_id, score, tiles) VALUES (?, ?, ?)",
        user_id,
        score,
        tiles
    )
    .execute(&ctx.data().database)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_POINT_POSITION: &[usize; 16] = &[10, 0, 14, 9, 7, 3, 12, 5, 2, 8, 6, 13, 1, 4, 15, 11];

    #[test]
    fn perfect_mixu_score() {
        let positions = MIKU_POSITIONS;
        let points = count_points(&positions);
        assert_eq!(points, 40);
    }

    #[test]
    fn awful_mixu_score() {
        let points = count_points(NO_POINT_POSITION);
        assert_eq!(points, 0);
    }

    #[test]
    fn one_row_and_one_col_neighbor() {
        let positions = [9, 15, 16, 2, 11, 10, 12, 1, 8, 4, 14, 5, 6, 3, 7, 13];
        let points = count_points(&positions);
        assert_eq!(points, 2);
    }

    #[test]
    fn no_points_for_wrapping_row_neighbors() {
        let positions = [10, 0, 14, 9, 7, 3, 4, 8, 2, 5, 6, 13, 1, 12, 15, 11];
        let points = count_points(&positions);
        assert_eq!(points, 1);
    }

    #[test]
    fn connection_bottom_right_corner() {
        let positions = [5, 12, 0, 9, 10, 8, 6, 4, 7, 1, 3, 11, 2, 13, 14, 15];
        let points = count_points(&positions);
        assert_eq!(points, 8);
    }
}
