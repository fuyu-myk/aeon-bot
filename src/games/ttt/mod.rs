pub mod board;
pub mod commands;
pub mod interactions;
pub mod qlearning;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use poise::serenity_prelude::{ChannelId, GuildId, MessageId, UserId};
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton};
use tokio::sync::Mutex;

use board::Board;

use crate::games::ttt::board::Cell;

/// Who an observer is betting on in a PvP game
#[derive(Clone, Debug, PartialEq)]
pub enum BetTarget {
    Player1,
    Player2,
    Draw,
}

impl BetTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "p1" => Some(BetTarget::Player1),
            "p2" => Some(BetTarget::Player2),
            "draw" => Some(BetTarget::Draw),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BetTarget::Player1 => "❌ (Player 1)",
            BetTarget::Player2 => "⭕ (Player 2)",
            BetTarget::Draw => "Draw",
        }
    }
}

/// A single bet placed by an observer on a PvP game
#[derive(Clone, Debug)]
pub struct Bet {
    pub target: BetTarget,
    /// Points wagered (currently fixed at 10)
    pub amount: i64,
}

/// An active tic-tac-toe game session
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct TttGame {
    pub game_id: String,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    /// Message ID of the board message (used to edit it in-place)
    pub message_id: MessageId,
    pub board: Board,
    /// Player 1 always plays as X (the one who started or challenged)
    pub player1_id: UserId,
    /// Player 2 plays as O; `None` means the bot is the opponent
    pub player2_id: Option<UserId>,
    /// 1 = it is player 1's turn, 2 = it is player 2/bot's turn
    pub current_turn: u8,
    /// `true` when both sides are human players
    pub is_pvp: bool,
    /// (state_before_move, action) pairs recorded for the bot's TD update
    pub bot_history: Vec<(String, usize)>,
    /// Observer bets: bettor UserId → Bet
    pub bets: HashMap<UserId, Bet>,
    /// Points each player has staked in the PvP wager (0 = no wager)
    pub wager: i64,
    pub last_activity: Instant,
    /// Total bot games at the moment this game started – used to
    /// calculate the experience-tiered point reward
    pub bot_games_at_start: i64,
}

/// A challenge waiting for the opponent to accept or decline
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PendingChallenge {
    pub game_id: String,
    pub challenger_id: UserId,
    pub opponent_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    /// Message ID of the Accept / Decline prompt
    pub message_id: MessageId,
    pub wager: i64,
    pub created_at: Instant,
}

pub type TttGames = Arc<Mutex<HashMap<String, TttGame>>>;
pub type TttChallenges = Arc<Mutex<HashMap<String, PendingChallenge>>>;

// ── Point-reward tiers ────────────────────────────────────────────────────────

/// Returns the bonus points a player earns for defeating the bot, scaled by
/// the bot's experience at the time the game started
pub fn points_for_beating_bot(bot_games_at_start: i64) -> i64 {
    match bot_games_at_start {
        0..=49 => 5,
        50..=199 => 10,
        200..=499 => 15,
        _ => 25,
    }
}

// ── Board rendering ───────────────────────────────────────────────────────────

/// Builds the 3×3 button grid for the current board state
pub fn make_board_components(board: &Board, game_id: &str, disabled: bool) -> Vec<CreateActionRow> {
    (0..3_usize)
        .map(|row| {
            let buttons: Vec<CreateButton> = (0..3_usize)
                .map(|col| {
                    let pos = row * 3 + col;
                    let cell = board.get(pos);
                    let (label, style, cell_disabled) = match cell {
                        Cell::Empty => ("⬜", ButtonStyle::Secondary, false),
                        Cell::X => ("❌", ButtonStyle::Primary, true),
                        Cell::O => ("⭕", ButtonStyle::Danger, true),
                    };

                    CreateButton::new(format!("ttt_move_{}_{}", game_id, pos))
                        .label(label)
                        .style(style)
                        .disabled(disabled || cell_disabled)
                })
                .collect();

            CreateActionRow::Buttons(buttons)
        })
        .collect()
}

/// Builds the observer-betting row appended below the board for PvP games
pub fn make_bet_row(game_id: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("ttt_bet_{}_p1", game_id))
            .label("Bet ❌ 10pt")
            .style(ButtonStyle::Primary),
        CreateButton::new(format!("ttt_bet_{}_p2", game_id))
            .label("Bet ⭕ 10pt")
            .style(ButtonStyle::Danger),
        CreateButton::new(format!("ttt_bet_{}_draw", game_id))
            .label("Bet Draw 10pt")
            .style(ButtonStyle::Secondary),
    ])
}

/// Accept / Decline prompt sent to the channel when a challenge is issued
pub fn make_challenge_components(game_id: &str) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("ttt_accept_{}", game_id))
            .label("Accept")
            .style(ButtonStyle::Success),
        CreateButton::new(format!("ttt_decline_{}", game_id))
            .label("Decline")
            .style(ButtonStyle::Danger),
    ])]
}

/// Human-readable status line shown above the board
pub fn game_status(
    board: &Board,
    p1_id: UserId,
    p2_id: Option<UserId>,
    current_turn: u8,
    bot_games: i64,
    is_pvp: bool,
    wager: i64,
) -> String {
    use board::GameResult;

    let p1_mention = format!("<@{}>", p1_id);
    let p2_label = p2_id
        .map(|id| format!("<@{}>", id))
        .unwrap_or_else(|| "aeon-bot".to_string());

    let header = if is_pvp {
        if wager > 0 {
            format!(
                "⚔️ **Tic-Tac-Toe** | {} ❌ vs {} ⭕  |  Wager: {} pts each",
                p1_mention, p2_label, wager
            )
        } else {
            format!("⚔️ **Tic-Tac-Toe** | {} ❌ vs {} ⭕", p1_mention, p2_label)
        }
    } else {
        let exp_label = match bot_games {
            0..=49 => format!("🐣 Novice ({} games)", bot_games),
            50..=199 => format!("🔰 Apprentice ({} games)", bot_games),
            200..=499 => format!("⚔️ Veteran ({} games)", bot_games),
            _ => format!("💀 Expert ({} games)", bot_games),
        };

        format!(
            "🎮 **Tic-Tac-Toe** | {} ❌ vs aeon-bot ⭕  |  Bot: {}",
            p1_mention, exp_label
        )
    };

    let turn_line = match board.result() {
        GameResult::InProgress => {
            if current_turn == 1 {
                format!("Turn: ❌ {}", p1_mention)
            } else {
                format!("Turn: ⭕ {}", p2_label)
            }
        }
        GameResult::Win(Cell::X) => format!("🏆 {} (❌) wins!", p1_mention),
        GameResult::Win(Cell::O) => format!("🏆 {} (⭕) wins!", p2_label),
        GameResult::Win(_) | GameResult::Draw => "🤝 It's a draw!".to_string(),
    };

    format!("{}\n{}", header, turn_line)
}

// ── Database helpers ──────────────────────────────────────────────────────────

async fn get_bot_total_games(db: &sqlx::SqlitePool) -> i64 {
    use sqlx::Row;

    sqlx::query("SELECT COALESCE(total_games, 0) AS total_games FROM ttt_bot_stats WHERE id = 1")
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|row| row.get::<i64, _>("total_games"))
        .unwrap_or(0)
}

async fn ensure_user_row(db: &sqlx::SqlitePool, user_id: UserId, guild_id: GuildId) {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
         VALUES (?, ?, 'Unknown', 0, 0)",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .execute(db)
    .await;
}
