use poise::serenity_prelude as serenity;
use serenity::builder::{
    CreateActionRow, CreateInputText, CreateInteractionResponse, CreateModal,
};
use serenity::all::InputTextStyle;
use sqlx::Row;


// ── Discord modal helpers ─────────────────────────────────────────────────────

/// Build a "place your bet" modal interaction response
///
/// `modal_id` becomes the modal's `custom_id`; callers should encode all game
/// context needed for the submission handler into this string
pub fn create_bet_modal(modal_id: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::Modal(
        CreateModal::new(modal_id, "Place your bet").components(vec![
            CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "Amount to wager", "wager_amount")
                    .placeholder("Enter a number, e.g. 10")
                    .min_length(1)
                    .required(true),
            ),
        ]),
    )
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