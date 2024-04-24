use std::{str::FromStr, time::Duration};

use crate::{
    common::{
        ephemeral_interaction_response, send_interaction_update, send_message_with_row, Score,
    },
    Context,
};
use anyhow::{bail, Result};
use poise::serenity_prelude::{ButtonStyle, InteractionResponseType, ReactionType};
use serenity::{
    builder::CreateActionRow, collector::ComponentInteractionCollectorBuilder, futures::StreamExt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Weapon {
    Rock,
    Paper,
    Scissors,
}

impl Weapon {
    fn compare(self, other: Weapon) -> Score {
        use Weapon::*;
        match (self, other) {
            (Rock, Paper) | (Paper, Scissors) | (Scissors, Rock) => Score::Loss,
            (Paper, Rock) | (Scissors, Paper) | (Rock, Scissors) => Score::Win,
            _ => Score::Draw,
        }
    }

    fn to_str(self) -> &'static str {
        match self {
            Weapon::Rock => "rock",
            Weapon::Paper => "paper",
            Weapon::Scissors => "scissors",
        }
    }
}

impl FromStr for Weapon {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        let choice = match s {
            "rps-rock" => Self::Rock,
            "rps-paper" => Self::Paper,
            "rps-scissors" => Self::Scissors,
            _ => bail!("Invalid weapon choice"),
        };
        Ok(choice)
    }
}

/// Challenge someone to a rock paper scissors battle
#[poise::command(slash_command)]
pub async fn rps(ctx: Context<'_>) -> Result<()> {
    let challenger = ctx.author();
    let initial_msg = format!("{challenger} is looking for a rock-paper-scissors opponent!");
    let first_message = send_message_with_row(ctx, initial_msg, create_accept_button()).await?;

    while let Some(interaction) = first_message
        .message()
        .await?
        .await_component_interaction(ctx)
        .timeout(Duration::from_secs(600))
        .await
    {
        if interaction.data.custom_id != "rps-btn" {
            continue;
        }

        if interaction.user.id == challenger.id {
            ephemeral_interaction_response(&ctx, interaction, "You cannot fight yourself.").await?;
            continue;
        }

        let accepter = interaction.user.clone();
        let weapon_request = "Choose your weapon!";
        let row = create_weapons_buttons();

        let (challenger_msg, _) = tokio::try_join!(
            ctx.send(|f| {
                f.content(weapon_request)
                    .ephemeral(true)
                    .components(|c| c.set_action_row(row.clone()))
            }),
            interaction.create_interaction_response(ctx, |r| {
                r.interaction_response_data(|d| {
                    d.content(weapon_request)
                        .ephemeral(true)
                        .components(|c| c.set_action_row(row.clone()))
                })
            }),
        )?;

        let (challenger_msg, accepter_msg) = tokio::try_join!(
            challenger_msg.message(),
            interaction.get_interaction_response(ctx)
        )?;

        let (challenger_choice, accepter_choice) = tokio::try_join!(
            get_user_weapon_choice(ctx, challenger_msg.id.0, challenger.id.0),
            get_user_weapon_choice(ctx, accepter_msg.id.0, accepter.id.0)
        )?;

        let mut end_msg = format!(
            "{challenger} picks {}, {accepter} picks {}\n",
            challenger_choice.to_str(),
            accepter_choice.to_str()
        );
        end_msg.push_str(&match challenger_choice.compare(accepter_choice) {
            Score::Win => format!("{challenger} wins!"),
            Score::Loss => format!("{accepter} wins!"),
            Score::Draw => "It's a draw!".to_owned(),
        });

        first_message
            .edit(ctx, |m| m.content(end_msg).components(|c| c))
            .await?;

        return Ok(());
    }

    first_message
        .edit(ctx, |m| {
            m.content(format!("Nobody was brave enough to challenge {challenger}"))
                .components(|c| c)
        })
        .await?;

    Ok(())
}

async fn get_user_weapon_choice(
    ctx: Context<'_>,
    message_id: u64,
    author_id: u64,
) -> Result<Weapon> {
    let mut collector = ComponentInteractionCollectorBuilder::new(ctx)
        .message_id(message_id)
        .timeout(std::time::Duration::from_secs(600))
        .collect_limit(1)
        .filter(move |f| {
            f.user.id.0 == author_id
                && ["rps-rock", "rps-paper", "rps-scissors"].contains(&f.data.custom_id.as_str())
        })
        .build();

    let weapon_button_interaction = collector
        .next()
        .await
        .ok_or(anyhow::anyhow!("Button press error"))?;

    send_interaction_update(ctx, &weapon_button_interaction, "Great choice!").await?;
    Weapon::from_str(&weapon_button_interaction.data.custom_id)
}

fn create_accept_button() -> CreateActionRow {
    let mut row = CreateActionRow::default();
    row.create_button(|f| {
        f.custom_id("rps-btn")
            .emoji('💪')
            .label("Accept Battle".to_string())
            .style(ButtonStyle::Primary)
    });

    row
}

fn create_weapons_buttons() -> CreateActionRow {
    let mut row = CreateActionRow::default();
    row.create_button(|f| {
        f.custom_id("rps-rock")
            .emoji('🪨')
            .label("Rock")
            .style(ButtonStyle::Primary)
    });
    row.create_button(|f| {
        f.custom_id("rps-paper")
            .emoji('🧻')
            .label("Paper")
            .style(ButtonStyle::Primary)
    });
    row.create_button(|f| {
        f.custom_id("rps-scissors")
            .emoji(ReactionType::Unicode("✂️".to_owned()))
            .label("Scissors")
            .style(ButtonStyle::Primary)
    });

    row
}
