use std::sync::atomic::{AtomicI64, Ordering};

use crate::{Context, Result};

use anyhow::bail;
use chrono::Utc;
use rand::seq::SliceRandom;
use serde::Deserialize;
use tokio::sync::{OnceCell, RwLock};
use crate::common::uwuify;

static QUOTES: OnceCell<RwLock<Vec<Quote>>> =
    OnceCell::const_new_with(RwLock::const_new(Vec::new()));
static QUOTES_LAST_UPDATED: AtomicI64 = AtomicI64::new(0);

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct QuoteResponse {
    error: bool,
    #[serde(rename = "type")]
    kind: usize,
    total: usize,
    data: Vec<Quote>,
}

#[derive(Debug, Deserialize)]
pub struct Quote {
    id: u64,
    body: String,
}

/// Get a random or specific quote
#[poise::command(slash_command, prefix_command)]
pub async fn quote(
    ctx: Context<'_>,
    #[description = "Quote ID"] quote_id: Option<u64>,
) -> Result<()> {
    let message = match generate_message(quote_id).await {
        Ok(message) => message,
        Err(e) => e.to_string(),
    };

    ctx.say(message).await?;

    Ok(())
}

/// Get a wandom ow specific quote
#[poise::command(slash_command, prefix_command)]
pub async fn quwuote(
    ctx: Context<'_>,
    #[description = "Quote ID"] quote_id: Option<u64>,
) -> Result<()> {
    let message = match generate_message(quote_id).await {
        Ok(message) => uwuify(&message),
        Err(e) =>uwuify(&e.to_string()),
    };

    ctx.say(message).await?;

    Ok(())
}

async fn generate_message(quote_id: Option<u64>) -> Result<String> {
    if has_cache_expired() {
        update_quotes().await;
    }

    let quotes = QUOTES
        .get()
        .expect("Quotes should be initialized in the bot setup function")
        .read()
        .await;

    if quotes.is_empty() {
        bail!("Could not retrieve any quotes");
    }

    let quote = choose_quote(&quotes, quote_id)?;

    Ok(format!("[{}] {}", quote.id, quote.body))
}

fn choose_quote(quotes: &[Quote], quote_id: Option<u64>) -> Result<&Quote> {
    let quote = match quote_id {
        Some(id) => quotes
            .iter()
            .find(|q| q.id == id)
            .ok_or(anyhow::anyhow!("Unable to find quote #{id}"))?,
        None => {
            let mut rng = rand::thread_rng();
            quotes.choose(&mut rng).expect("Quotes to not be empty")
        }
    };

    Ok(quote)
}

async fn update_quotes() {
    let response = match fetch_quotes().await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Failed to fetch quotes: {e:?}");
            return;
        }
    };

    let mut quotes = QUOTES
        .get()
        .expect("Quotes should be initialized in the bot setup function")
        .write()
        .await;
    *quotes = response.data;
    QUOTES_LAST_UPDATED.store(Utc::now().timestamp(), Ordering::Release)
}

fn has_cache_expired() -> bool {
    let now = Utc::now().timestamp();
    let cache_duration = chrono::Duration::minutes(30).num_milliseconds();
    let last_updated = QUOTES_LAST_UPDATED.load(Ordering::Acquire);

    now > last_updated + cache_duration
}

async fn fetch_quotes() -> std::result::Result<QuoteResponse, reqwest::Error> {
    reqwest::get("https://api.bufutda.com/bot/quote?channel=bananasaurus_rex")
        .await?
        .json::<QuoteResponse>()
        .await
}
