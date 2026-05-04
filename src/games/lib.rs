use ::serenity::all::{ComponentInteraction, CreateInteractionResponseMessage, ModalInteraction};
use poise::serenity_prelude as serenity;
use serenity::all::InputTextStyle;
use serenity::builder::{CreateActionRow, CreateInputText, CreateInteractionResponse, CreateModal};
use sqlx::Row;

use crate::games::ttt::BetTarget;

// ── Utility helpers ───────────────────────────────────────────────────────────

pub fn generate_game_id() -> String {
    use rand::Rng;
    format!("{:08x}", rand::thread_rng().r#gen::<u32>())
}

/// How long an idle game is kept before it is treated as abandoned
pub const GAME_TIMEOUT_SECS: u64 = 600;

/// Sends an ephemeral message in response to a component interaction
pub async fn ephemeral_reply(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    text: &str,
) {
    let _ = component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .ephemeral(true),
            ),
        )
        .await;
}

/// Sends an ephemeral message in response to a modal submission
pub async fn modal_ephemeral_reply(ctx: &serenity::Context, modal: &ModalInteraction, text: &str) {
    let _ = modal
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .ephemeral(true),
            ),
        )
        .await;
}

/// Parse `ttt_move_{game_id}_{pos}` → `(game_id, pos)`
pub fn parse_move_id(custom_id: &str) -> Option<(String, usize)> {
    let rest = custom_id.strip_prefix("ttt_move_")?;
    let (game_id, pos_str) = rest.rsplit_once('_')?;
    let pos = pos_str.parse().ok()?;
    Some((game_id.to_string(), pos))
}

/// Parse `ttt_bet_{game_id}_{target}` → `(game_id, target)`
pub fn parse_bet_id(custom_id: &str) -> Option<(String, BetTarget)> {
    let rest = custom_id.strip_prefix("ttt_bet_")?;
    let (game_id, target_str) = rest.rsplit_once('_')?;
    let target = BetTarget::from_str(target_str)?;

    Some((game_id.to_string(), target))
}

// ── Discord modal helpers ─────────────────────────────────────────────────────

/// Build a "place your bet" modal interaction response
///
/// `modal_id` becomes the modal's `custom_id`; callers should encode all game
/// context needed for the submission handler into this string
pub fn create_bet_modal(modal_id: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::Modal(CreateModal::new(modal_id, "Place your bet").components(
        vec![CreateActionRow::InputText(
            CreateInputText::new(InputTextStyle::Short, "Amount to wager", "wager_amount")
                .placeholder("Enter a number, e.g. 10")
                .min_length(1)
                .required(true),
        )],
    ))
}

pub fn update_message(
    content: impl Into<String>,
    row: CreateActionRow,
) -> CreateInteractionResponse {
    CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .content(content)
            .components(vec![row]),
    )
}

/// Parse `ttt_bet_modal_{game_id}_{target}` → `(game_id, target)`
pub fn parse_bet_modal_id(custom_id: &str) -> Option<(String, BetTarget)> {
    let rest = custom_id.strip_prefix("ttt_bet_modal_")?;
    let (game_id, target_str) = rest.rsplit_once('_')?;
    let target = BetTarget::from_str(target_str)?;

    Some((game_id.to_string(), target))
}

/// Extract a named text-input value from a modal submission's components
pub fn extract_modal_value<'a>(modal: &'a ModalInteraction, field_id: &str) -> Option<&'a str> {
    modal
        .data
        .components
        .iter()
        .flat_map(|row| row.components.iter())
        .find_map(|comp| match comp {
            serenity::ActionRowComponent::InputText(input) if input.custom_id == field_id => {
                input.value.as_deref()
            }
            _ => None,
        })
}

// ── Shared point database helpers ─────────────────────────────────────────────

/// Fetch `total_points` for a user in a guild. Returns 0 if no row exists
pub async fn get_user_points(
    db: &sqlx::SqlitePool,
    user_id: serenity::UserId,
    guild_id: serenity::GuildId,
) -> i64 {
    sqlx::query(
        "SELECT COALESCE(total_points, 0) AS total_points
         FROM users WHERE user_id = ? AND guild_id = ?",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .map(|row| row.get::<i64, _>("total_points"))
    .unwrap_or(0)
}

/// Add (or subtract when negative) points for a user in a guild.
/// Creates the user row silently if it does not yet exist
pub async fn add_points(
    db: &sqlx::SqlitePool,
    user_id: serenity::UserId,
    guild_id: serenity::GuildId,
    amount: i64,
) {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
         VALUES (?, ?, 'Unknown', 0, 0)",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .execute(db)
    .await;

    let _ = sqlx::query(
        "UPDATE users SET total_points = total_points + ?, last_updated = CURRENT_TIMESTAMP
         WHERE user_id = ? AND guild_id = ?",
    )
    .bind(amount)
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .execute(db)
    .await;
}

/// Subtract points from a user. Thin wrapper around [`add_points`]
pub async fn deduct_points(
    db: &sqlx::SqlitePool,
    user_id: serenity::UserId,
    guild_id: serenity::GuildId,
    amount: i64,
) {
    add_points(db, user_id, guild_id, -amount).await;
}
