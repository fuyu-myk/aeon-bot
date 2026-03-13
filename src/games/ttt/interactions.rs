use std::collections::HashMap;
use std::time::Instant;

use poise::serenity_prelude as serenity;
use serenity::{
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    model::application::{ComponentInteraction, ModalInteraction},
};
use sqlx::Row;

use crate::events::GlobalTracker;
use crate::games::lib as games_lib;
use super::{
    generate_game_id, points_for_beating_bot, Bet, BetTarget, TttGame, GAME_TIMEOUT_SECS,
};
use super::board::{Board, Cell, GameResult};
use super::commands::{game_status, make_bet_row, make_board_components};
use super::qlearning::QLearner;


// ── Public entry points ───────────────────────────────────────────────────────

/// Routes every `ttt_*` component interaction to the correct handler
pub async fn handle_ttt_interaction(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let id = component.data.custom_id.clone();

    if id.starts_with("ttt_move_") {
        handle_move(ctx, component, data).await;
    } else if id.starts_with("ttt_accept_") {
        handle_accept(ctx, component, data).await;
    } else if id.starts_with("ttt_decline_") {
        handle_decline(ctx, component, data).await;
    } else if id.starts_with("ttt_bet_") {
        handle_bet(ctx, component, data).await;
    }
}

/// Routes every `ttt_*` modal submission to the correct handler
pub async fn handle_ttt_modal(
    ctx: &serenity::Context,
    modal: &ModalInteraction,
    data: &GlobalTracker,
) {
    if modal.data.custom_id.starts_with("ttt_bet_modal_") {
        handle_bet_modal_submit(ctx, modal, data).await;
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Sends an ephemeral message in response to a component interaction
async fn ephemeral_reply(ctx: &serenity::Context, component: &ComponentInteraction, text: &str) {
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
async fn modal_ephemeral_reply(ctx: &serenity::Context, modal: &ModalInteraction, text: &str) {
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
fn parse_move_id(custom_id: &str) -> Option<(String, usize)> {
    let rest = custom_id.strip_prefix("ttt_move_")?;
    let (game_id, pos_str) = rest.rsplit_once('_')?;
    let pos = pos_str.parse().ok()?;
    Some((game_id.to_string(), pos))
}

/// Parse `ttt_bet_{game_id}_{target}` → `(game_id, target)`
fn parse_bet_id(custom_id: &str) -> Option<(String, BetTarget)> {
    let rest = custom_id.strip_prefix("ttt_bet_")?;
    let (game_id, target_str) = rest.rsplit_once('_')?;
    let target = BetTarget::from_str(target_str)?;

    Some((game_id.to_string(), target))
}

/// Parse `ttt_bet_modal_{game_id}_{target}` → `(game_id, target)`
fn parse_bet_modal_id(custom_id: &str) -> Option<(String, BetTarget)> {
    let rest = custom_id.strip_prefix("ttt_bet_modal_")?;
    let (game_id, target_str) = rest.rsplit_once('_')?;
    let target = BetTarget::from_str(target_str)?;

    Some((game_id.to_string(), target))
}

/// Extract a named text-input value from a modal submission's components
fn extract_modal_value<'a>(modal: &'a ModalInteraction, field_id: &str) -> Option<&'a str> {
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

// ── Move handler ──────────────────────────────────────────────────────────────

async fn handle_move(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let (game_id, pos) = match parse_move_id(&component.data.custom_id) {
        Some(v) => v,
        None => return,
    };

    let user_id = component.user.id;

    // ── Snapshot the game (release lock immediately) ──────────────────────
    let game_snapshot = {
        let mut games = data.ttt_games.lock().await;

        games.retain(|_, g| g.last_activity.elapsed().as_secs() < GAME_TIMEOUT_SECS);

        match games.get(&game_id) {
            Some(g) => g.clone(),
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has expired or no longer exists.").await;
                return;
            }
        }
    };

    // ── Validate it's your turn ───────────────────────────────────────────
    let is_p1 = game_snapshot.player1_id == user_id;
    let is_p2 = game_snapshot.player2_id.map_or(false, |id| id == user_id);
    let is_bot_game = !game_snapshot.is_pvp;

    let allowed = (game_snapshot.current_turn == 1 && is_p1)
        || (game_snapshot.current_turn == 2 && (is_p2 || is_bot_game));

    if !allowed {
        if !is_p1 && !is_p2 {
            ephemeral_reply(ctx, component, "You're not in this game!").await;
        } else {
            ephemeral_reply(ctx, component, "It's not your turn yet!").await;
        }
        return;
    }

    if pos >= 9 || game_snapshot.board.get(pos) != Cell::Empty {
        ephemeral_reply(ctx, component, "That cell is already taken!").await;
        return;
    }

    // ── Apply player's move ───────────────────────────────────────────────
    let player_cell = if game_snapshot.current_turn == 1 { Cell::X } else { Cell::O };
    let board_after_player = game_snapshot.board.with_move(pos, player_cell);
    let player_result = board_after_player.result();

    // ── If bot game and still in progress, let bot move ───────────────────
    let (final_board, bot_history_entry, game_result) = if is_bot_game
        && player_result == GameResult::InProgress
    {
        let available = board_after_player.available_moves();
        if available.is_empty() {
            (board_after_player.clone(), None, board_after_player.result())
        } else {
            let q = QLearner::new();
            let bot_state = board_after_player.to_state_key();
            let bot_pos = q
                .select_action(&data.db, &bot_state, &available, game_snapshot.bot_games_at_start)
                .await;

            let board_after_bot = board_after_player.with_move(bot_pos, Cell::O);
            let result = board_after_bot.result();
            (board_after_bot, Some((bot_state, bot_pos)), result)
        }
    } else {
        (board_after_player, None, player_result)
    };

    // ── Work out next turn ────────────────────────────────────────────────
    let next_turn = if game_snapshot.is_pvp {
        if game_snapshot.current_turn == 1 { 2 } else { 1 }
    } else {
        1
    };

    let is_over = game_result != GameResult::InProgress;

    let mut new_bot_history = game_snapshot.bot_history.clone();
    if let Some(entry) = bot_history_entry {
        new_bot_history.push(entry);
    }

    // ── Build Discord response ────────────────────────────────────────────
    let status = game_status(
        &final_board,
        game_snapshot.player1_id,
        game_snapshot.player2_id,
        next_turn,
        game_snapshot.bot_games_at_start,
        game_snapshot.is_pvp,
        game_snapshot.wager,
    );

    let mut components = make_board_components(&final_board, &game_id, is_over);
    if game_snapshot.is_pvp && !is_over {
        components.push(make_bet_row(&game_id));
    }

    let _ = component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(&status)
                    .components(components),
            ),
        )
        .await;

    // ── Update or remove the game record ──────────────────────────────────
    if is_over {
        data.ttt_games.lock().await.remove(&game_id);
        finalize_game(ctx, data, &game_snapshot, &new_bot_history, game_result).await;
    } else {
        let mut games = data.ttt_games.lock().await;

        if let Some(game) = games.get_mut(&game_id) {
            game.board = final_board;
            game.current_turn = next_turn;
            game.bot_history = new_bot_history;
            game.last_activity = Instant::now();
        }
    }
}

// ── Challenge accept / decline ────────────────────────────────────────────────

async fn handle_accept(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = match component.data.custom_id.strip_prefix("ttt_accept_") {
        Some(id) => id.to_string(),
        None => return,
    };

    let user_id = component.user.id;

    let challenge = {
        let challenges = data.ttt_challenges.lock().await;
        match challenges.get(&game_id) {
            Some(c) => c.clone(),
            None => {
                ephemeral_reply(ctx, component, "This challenge has expired.").await;
                return;
            }
        }
    };

    if user_id != challenge.opponent_id {
        ephemeral_reply(ctx, component, "This challenge is not for you!").await;
        return;
    }

    if challenge.wager > 0 {
        let challenger_pts =
            games_lib::get_user_points(&data.db, challenge.challenger_id, challenge.guild_id).await;
        let opponent_pts =
            games_lib::get_user_points(&data.db, challenge.opponent_id, challenge.guild_id).await;

        if challenger_pts < challenge.wager {
            delete_challenge(ctx, component, data, &game_id, &format!(
                "<@{}> no longer has enough points to cover the wager. Challenge cancelled.",
                challenge.challenger_id
            )).await;
            return;
        }
        if opponent_pts < challenge.wager {
            delete_challenge(ctx, component, data, &game_id, &format!(
                "<@{}>, you don't have enough points to cover the wager of {} pts.",
                challenge.opponent_id, challenge.wager
            )).await;
            return;
        }

        games_lib::deduct_points(&data.db, challenge.challenger_id, challenge.guild_id, challenge.wager).await;
        games_lib::deduct_points(&data.db, challenge.opponent_id, challenge.guild_id, challenge.wager).await;
    }

    data.ttt_challenges.lock().await.remove(&game_id);

    let board = Board::new();
    let new_game_id = generate_game_id();
    let bot_games = get_bot_total_games(&data.db).await;

    let content = game_status(
        &board,
        challenge.challenger_id,
        Some(challenge.opponent_id),
        1,
        bot_games,
        true,
        challenge.wager,
    );

    let mut components = make_board_components(&board, &new_game_id, false);
    components.push(make_bet_row(&new_game_id));

    let _ = component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(&content)
                    .components(components),
            ),
        )
        .await;

    let message_id = component.message.id.into();

    let game = TttGame {
        game_id: new_game_id.clone(),
        guild_id: challenge.guild_id,
        channel_id: challenge.channel_id,
        message_id,
        board,
        player1_id: challenge.challenger_id,
        player2_id: Some(challenge.opponent_id),
        current_turn: 1,
        is_pvp: true,
        bot_history: Vec::new(),
        bets: HashMap::new(),
        wager: challenge.wager,
        last_activity: Instant::now(),
        bot_games_at_start: bot_games,
    };

    data.ttt_games.lock().await.insert(new_game_id, game);
}

async fn delete_challenge(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
    game_id: &str,
    message: &str,
) {
    data.ttt_challenges.lock().await.remove(game_id);
    let _ = component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(message)
                    .components(vec![]),
            ),
        )
        .await;
}

async fn handle_decline(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = match component.data.custom_id.strip_prefix("ttt_decline_") {
        Some(id) => id.to_string(),
        None => return,
    };

    let challenge = {
        let challenges = data.ttt_challenges.lock().await;
        match challenges.get(&game_id) {
            Some(c) => c.clone(),
            None => {
                ephemeral_reply(ctx, component, "This challenge has already expired.").await;
                return;
            }
        }
    };

    if component.user.id != challenge.opponent_id {
        ephemeral_reply(ctx, component, "Only the challenged player can decline!").await;
        return;
    }

    delete_challenge(
        ctx,
        component,
        data,
        &game_id,
        &format!(
            "<@{}> declined the challenge from <@{}>.",
            challenge.opponent_id, challenge.challenger_id
        ),
    )
    .await;
}

// ── Observer betting ──────────────────────────────────────────────────────────

/// Called when an observer clicks a bet button. Pre-validates, then shows the
/// modal to collect the wager amount
async fn handle_bet(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let (game_id, target) = match parse_bet_id(&component.data.custom_id) {
        Some(v) => v,
        None => return,
    };

    let user_id = component.user.id;

    // Pre-validate before showing modal
    let game_snapshot = {
        let games = data.ttt_games.lock().await;
        match games.get(&game_id) {
            Some(g) => g.clone(),
            None => {
                ephemeral_reply(ctx, component, "This game has already ended.").await;
                return;
            }
        }
    };

    if game_snapshot.player1_id == user_id || game_snapshot.player2_id == Some(user_id) {
        ephemeral_reply(ctx, component, "Players cannot bet on their own game!").await;
        return;
    }

    if game_snapshot.bets.contains_key(&user_id) {
        ephemeral_reply(ctx, component, "You've already placed a bet on this game!").await;
        return;
    }

    // Encode game context in the modal custom_id
    let target_str = match &target {
        BetTarget::Player1 => "p1",
        BetTarget::Player2 => "p2",
        BetTarget::Draw => "draw",
    };
    let modal_id = format!("ttt_bet_modal_{}_{}", game_id, target_str);

    let _ = component
        .create_response(&ctx.http, games_lib::create_bet_modal(modal_id))
        .await;
}

/// Called when the bet modal is submitted. Validates the amount, deducts
/// points, and records the bet
async fn handle_bet_modal_submit(
    ctx: &serenity::Context,
    modal: &ModalInteraction,
    data: &GlobalTracker,
) {
    let (game_id, target) = match parse_bet_modal_id(&modal.data.custom_id) {
        Some(v) => v,
        None => return,
    };

    let user_id = modal.user.id;

    // Extract and parse the wager amount
    let amount_str = extract_modal_value(modal, "wager_amount").unwrap_or("");
    let amount: i64 = match amount_str.trim().parse() {
        Ok(n) if n >= 1 => n,
        _ => {
            modal_ephemeral_reply(ctx, modal, "Please enter a valid positive number.").await;
            return;
        }
    };

    // Re-validate game state (it may have ended while the user filled in the modal)
    let game_snapshot = {
        let games = data.ttt_games.lock().await;
        match games.get(&game_id) {
            Some(g) => g.clone(),
            None => {
                modal_ephemeral_reply(ctx, modal, "This game has already ended.").await;
                return;
            }
        }
    };

    if game_snapshot.player1_id == user_id || game_snapshot.player2_id == Some(user_id) {
        modal_ephemeral_reply(ctx, modal, "Players cannot bet on their own game!").await;
        return;
    }

    if game_snapshot.bets.contains_key(&user_id) {
        modal_ephemeral_reply(ctx, modal, "You've already placed a bet on this game!").await;
        return;
    }

    let guild_id = game_snapshot.guild_id;

    // Ensure the user row exists before checking/deducting points
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
         VALUES (?, ?, 'Unknown', 0, 0)",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .execute(&data.db)
    .await;

    let points = games_lib::get_user_points(&data.db, user_id, guild_id).await;
    if points < amount {
        modal_ephemeral_reply(
            ctx,
            modal,
            &format!(
                "You only have {} points — not enough to wager {}.",
                points, amount
            ),
        )
        .await;
        return;
    }

    games_lib::deduct_points(&data.db, user_id, guild_id, amount).await;

    {
        let mut games = data.ttt_games.lock().await;
        if let Some(game) = games.get_mut(&game_id) {
            game.bets.insert(user_id, Bet { target: target.clone(), amount });
        }
    }

    modal_ephemeral_reply(
        ctx,
        modal,
        &format!(
            "Bet of **{}** points placed on **{}**! You'll get 2× back if correct.",
            amount,
            target.label()
        ),
    )
    .await;
}

// ── Game finalization ─────────────────────────────────────────────────────────

async fn finalize_game(
    _ctx: &serenity::Context,
    data: &GlobalTracker,
    game: &TttGame,
    bot_history: &[(String, usize)],
    result: GameResult,
) {
    let db = &data.db;

    let (p1_won, p2_won, is_draw) = match result {
        GameResult::Win(Cell::X) => (true, false, false),
        GameResult::Win(Cell::O) => (false, true, false),
        GameResult::Draw => (false, false, true),
        _ => return,
    };

    if !game.is_pvp {
        // ── Bot game ──────────────────────────────────────────────────────
        let (bot_wins, bot_losses, bot_draws) = match result {
            GameResult::Win(Cell::O) => (1i64, 0, 0),
            GameResult::Win(Cell::X) => (0, 1, 0),
            _ => (0, 0, 1i64),
        };

        let _ = sqlx::query(
            "INSERT INTO ttt_bot_stats (id, total_games, wins, losses, draws)
             VALUES (1, 1, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               total_games = total_games + 1,
               wins   = wins   + excluded.wins,
               losses = losses + excluded.losses,
               draws  = draws  + excluded.draws",
        )
        .bind(bot_wins)
        .bind(bot_losses)
        .bind(bot_draws)
        .execute(db)
        .await;

        let final_reward = match result {
            GameResult::Win(Cell::O) =>  1.0,
            GameResult::Win(Cell::X) => -1.0,
            _ =>  0.0,
        };
        QLearner::new().td_update(db, bot_history, final_reward).await;

        if p1_won {
            let bonus = points_for_beating_bot(game.bot_games_at_start);
            games_lib::add_points(db, game.player1_id, game.guild_id, bonus).await;
        } else if is_draw {
            games_lib::add_points(db, game.player1_id, game.guild_id, 1).await;
        }

        upsert_player_stats(db, game.player1_id, game.guild_id, p1_won, p2_won, is_draw).await;
    } else {
        // ── PvP game ──────────────────────────────────────────────────────
        upsert_player_stats(db, game.player1_id, game.guild_id, p1_won, p2_won, is_draw).await;
        if let Some(p2) = game.player2_id {
            upsert_player_stats(db, p2, game.guild_id, p2_won, p1_won, is_draw).await;
        }

        if game.wager > 0 {
            let total = game.wager * 2;
            if p1_won {
                games_lib::add_points(db, game.player1_id, game.guild_id, total).await;
            } else if p2_won {
                if let Some(p2) = game.player2_id {
                    games_lib::add_points(db, p2, game.guild_id, total).await;
                }
            } else if is_draw {
                games_lib::add_points(db, game.player1_id, game.guild_id, game.wager).await;
                if let Some(p2) = game.player2_id {
                    games_lib::add_points(db, p2, game.guild_id, game.wager).await;
                }
            }
        }

        for (bettor_id, bet) in &game.bets {
            let bet_won = matches!(
                (&bet.target, p1_won, p2_won, is_draw),
                (BetTarget::Player1, true, _, _)
                | (BetTarget::Player2, _, true, _)
                | (BetTarget::Draw, _, _, true)
            );
            if bet_won {
                games_lib::add_points(db, *bettor_id, game.guild_id, bet.amount * 2).await;
            }
        }
    }
}

// ── TTT-specific database helpers ─────────────────────────────────────────────

async fn get_bot_total_games(db: &sqlx::SqlitePool) -> i64 {
    sqlx::query(
        "SELECT COALESCE(total_games, 0) AS total_games FROM ttt_bot_stats WHERE id = 1",
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .map(|row| row.get::<i64, _>("total_games"))
    .unwrap_or(0)
}

async fn upsert_player_stats(
    db: &sqlx::SqlitePool,
    user_id: serenity::UserId,
    guild_id: serenity::GuildId,
    won: bool,
    lost: bool,
    drew: bool,
) {
    let _ = sqlx::query(
        "INSERT INTO ttt_player_stats (user_id, guild_id, wins, losses, draws)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(user_id, guild_id) DO UPDATE SET
           wins   = wins   + excluded.wins,
           losses = losses + excluded.losses,
           draws  = draws  + excluded.draws",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .bind(if won  { 1i64 } else { 0 })
    .bind(if lost { 1i64 } else { 0 })
    .bind(if drew { 1i64 } else { 0 })
    .execute(db)
    .await;
}