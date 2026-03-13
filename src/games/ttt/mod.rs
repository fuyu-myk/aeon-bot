pub mod board;
pub mod qlearning;
pub mod commands;
pub mod interactions;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use poise::serenity_prelude::{UserId, GuildId, ChannelId, MessageId};

use board::Board;


/// How long an idle game is kept before it is treated as abandoned
pub const GAME_TIMEOUT_SECS: u64 = 600;

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
        0..=49    =>  5,
        50..=199  => 10,
        200..=499 => 15,
        _         => 25,
    }
}

// ── Utility helpers ───────────────────────────────────────────────────────────

pub fn generate_game_id() -> String {
    use rand::Rng;
    format!("{:08x}", rand::thread_rng().r#gen::<u32>())
}