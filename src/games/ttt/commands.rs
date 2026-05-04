use std::collections::HashMap;
use std::time::Instant;

use poise::serenity_prelude as serenity;
use sqlx::Row;

use super::board::Board;
use super::{PendingChallenge, TttGame};
use crate::events::GlobalTracker;
use crate::games::lib::{generate_game_id, get_user_points};
use crate::games::ttt::{
    ensure_user_row, game_status, get_bot_total_games, make_board_components,
    make_challenge_components,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, GlobalTracker, Error>;

/// Tic-Tac-Toe — play the bot, check stats, or challenge a friend
#[poise::command(
    slash_command,
    prefix_command,
    subcommands("play", "ttt_stats", "challenge")
)]
pub async fn ttt(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "**Tic-Tac-Toe commands:**\n\
         • `/ttt play` — play against aeon-bot\n\
         • `/ttt stats` — view aeon-bot's win/loss/draw record\n\
         • `/ttt challenge @user [wager]` — challenge another player",
    )
    .await?;

    Ok(())
}

/// Play Tic-Tac-Toe against the bot
#[poise::command(slash_command, prefix_command)]
pub async fn play(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };

    let player_id = ctx.author().id;

    // Prevent a player from having two simultaneous games in the same guild
    {
        let games = ctx.data().ttt_games.lock().await;
        let already_playing = games.values().any(|g| {
            g.guild_id == guild_id && (g.player1_id == player_id || g.player2_id == Some(player_id))
        });

        if already_playing {
            ctx.say("You already have an active Tic-Tac-Toe game in this server! Finish it first.")
                .await?;
            return Ok(());
        }
    }

    let bot_games = get_bot_total_games(&ctx.data().db).await;
    let game_id = generate_game_id();
    let board = Board::new();

    let status = game_status(&board, player_id, None, 1, bot_games, false, 0);
    let components = make_board_components(&board, &game_id, false);

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(status)
                .components(components),
        )
        .await?;

    let message = reply.message().await?;

    let game = TttGame {
        game_id: game_id.clone(),
        guild_id,
        channel_id: ctx.channel_id(),
        message_id: message.id,
        board,
        player1_id: player_id,
        player2_id: None,
        current_turn: 1,
        is_pvp: false,
        bot_history: Vec::new(),
        bets: HashMap::new(),
        wager: 0,
        last_activity: Instant::now(),
        bot_games_at_start: bot_games,
    };

    ctx.data().ttt_games.lock().await.insert(game_id, game);
    Ok(())
}

/// View the bot's Tic-Tac-Toe win/loss/draw record
#[poise::command(slash_command, prefix_command, rename = "stats")]
pub async fn ttt_stats(ctx: Context<'_>) -> Result<(), Error> {
    let db = &ctx.data().db;

    let row =
        sqlx::query("SELECT total_games, wins, losses, draws FROM ttt_bot_stats WHERE id = 1")
            .fetch_optional(db)
            .await?;

    let (total, wins, losses, draws) = match row {
        Some(r) => (
            r.get::<i64, _>("total_games"),
            r.get::<i64, _>("wins"),
            r.get::<i64, _>("losses"),
            r.get::<i64, _>("draws"),
        ),
        None => (0, 0, 0, 0),
    };

    let win_rate = if total > 0 {
        format!("{:.1}%", wins as f64 / total as f64 * 100.0)
    } else {
        "N/A".to_string()
    };

    let experience = match total {
        0..=49 => "🐣 Novice",
        50..=199 => "🔰 Apprentice",
        200..=499 => "⚔️ Veteran",
        _ => "💀 Expert",
    };

    let epsilon = crate::games::ttt::qlearning::QLearner::epsilon(total);

    ctx.say(format!(
        "**aeon-bot Tic-Tac-Toe Stats**\n\n\
         Experience: {}\n\
         Total games: {}\n\
         Wins: {}  |  Losses: {}  |  Draws: {}\n\
         Win rate: {}\n\
         Exploration rate (ε): {:.2} ({} makes {:.0}% random moves)\n\n\
         **Reward tiers for beating the bot:**\n\
         🐣 0–49 games → +5 pts\n\
         🔰 50–199 games → +10 pts\n\
         ⚔️ 200–499 games → +15 pts\n\
         💀 500+ games → +25 pts",
        experience,
        total,
        wins,
        losses,
        draws,
        win_rate,
        epsilon,
        "bot",
        epsilon * 100.0
    ))
    .await?;

    Ok(())
}

/// Challenge another user to Tic-Tac-Toe with an optional points wager
#[poise::command(slash_command, prefix_command)]
pub async fn challenge(
    ctx: Context<'_>,
    #[description = "The user to challenge"] opponent: serenity::User,
    #[description = "Points to wager (both players stake this amount, winner takes all)"]
    wager: Option<i64>,
) -> Result<(), Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };

    let challenger_id = ctx.author().id;

    if opponent.id == challenger_id {
        ctx.say("You can't challenge yourself!").await?;
        return Ok(());
    }
    if opponent.bot {
        ctx.say("You can't challenge a bot!\nUse `/ttt play` to play against aeon-bot.")
            .await?;
        return Ok(());
    }

    let wager = wager.unwrap_or(0).max(0);

    // Validate challenger has enough points for the wager
    if wager > 0 {
        ensure_user_row(&ctx.data().db, challenger_id, guild_id).await;
        let pts = get_user_points(&ctx.data().db, challenger_id, guild_id).await;

        if pts < wager {
            ctx.say(format!(
                "You don't have enough points to wager {} (you have {}).",
                wager, pts
            ))
            .await?;

            return Ok(());
        }
    }

    // Prevent already-active players from issuing a challenge
    {
        let games = ctx.data().ttt_games.lock().await;
        let busy = games.values().any(|g| {
            g.guild_id == guild_id
                && (g.player1_id == challenger_id
                    || g.player1_id == opponent.id
                    || g.player2_id == Some(challenger_id)
                    || g.player2_id == Some(opponent.id))
        });

        if busy {
            ctx.say("One of the players already has an active game. Finish it first!")
                .await?;
            return Ok(());
        }
    }

    let game_id = generate_game_id();

    let wager_text = if wager > 0 {
        format!(" for a wager of **{} pts** each", wager)
    } else {
        String::new()
    };

    let content = format!(
        "⚔️ <@{}> challenges <@{}>{}!\n<@{}> — do you accept?",
        challenger_id, opponent.id, wager_text, opponent.id
    );

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(content)
                .components(make_challenge_components(&game_id)),
        )
        .await?;

    let message = reply.message().await?;

    let challenge = PendingChallenge {
        game_id: game_id.clone(),
        challenger_id,
        opponent_id: opponent.id,
        guild_id,
        channel_id: ctx.channel_id(),
        message_id: message.id,
        wager,
        created_at: Instant::now(),
    };

    ctx.data()
        .ttt_challenges
        .lock()
        .await
        .insert(game_id, challenge);

    Ok(())
}
