use std::time::Instant;

use ::serenity::all::{CreateInteractionResponse, CreateInteractionResponseMessage};
use poise::serenity_prelude as serenity;
use serenity::model::application::ComponentInteraction;

use crate::{
    events::GlobalTracker,
    games::{
        bj::{
            BjGame, BjPhase, BjResolution, BjWinType, HandState, actionset_disabled,
            compute_available_actions, make_action_buttons, make_lobby_buttons,
        },
        lib::{
            GAME_TIMEOUT_SECS, add_points, create_bet_modal, deduct_points, ephemeral_reply,
            extract_modal_value, get_user_points, modal_ephemeral_reply, update_message,
        },
    },
};

fn extract_prefix(parts: Vec<&str>) -> String {
    let prefix_len = parts.len() - 1;
    parts[0..prefix_len].join("_")
}

/// Basic gate checks for player actions
async fn gate_checks(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    game_snapshot: &BjGame,
    user_id: serenity::UserId,
) -> Result<(), ()> {
    let is_in_game = game_snapshot.players.iter().any(|(p, _)| *p == user_id);
    let current_player = game_snapshot
        .players
        .get(game_snapshot.current)
        .map(|(id, _)| *id)
        .expect("Current player index should be valid");

    if !is_in_game {
        ephemeral_reply(ctx, component, "You're not a player in this game!").await;
        return Err(());
    }
    if current_player != user_id {
        ephemeral_reply(ctx, component, "It's not your turn!").await;
        return Err(());
    }
    if game_snapshot.current_phase != BjPhase::PlayerTurn {
        ephemeral_reply(ctx, component, "It's not your turn.").await;
        return Err(());
    }
    if game_snapshot.player_hands[game_snapshot.current].state != HandState::Active {
        ephemeral_reply(ctx, component, "Your hand is already resolved.").await;
        return Err(());
    }

    Ok(())
}

pub async fn apply_payouts(
    data: &GlobalTracker,
    game: &mut BjGame,
    payouts: &Vec<(usize, BjResolution, i64)>,
) {
    let db = &data.db;
    let guild_id = game.guild_id;
    for (player_idx, resolution, delta) in payouts {
        let (user_id, _) = *game.players.get(*player_idx).expect("Player not found");
        let wager = game.player_hands[*player_idx].wager;
        let refund = match resolution {
            BjResolution::PlayerWin(_) => wager + *delta, // wager back + profit
            BjResolution::Push => wager,                  // wager back, no profit
            BjResolution::Surrender => wager / 2,         // half wager back
            BjResolution::DealerWin(_) => 0,              // wager forfeited
        };

        if refund > 0 {
            add_points(db, user_id, guild_id, refund).await;
        }
    }

    game.current_phase = BjPhase::GameOver;
}

pub async fn update_bj_player_stats(
    data: &GlobalTracker,
    game: &BjGame,
    resolution: Vec<(usize, BjResolution, i64)>,
) {
    let db = &data.db;
    for (i, _) in game.player_hands.iter().enumerate() {
        let (user_id, _) = *game.players.get(i).expect("Player not found");

        let _ = sqlx::query(
            "INSERT INTO bj_player_stats (user_id, guild_id, wins, losses, pushes, surrenders, blackjacks, total_wagered, total_won)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, guild_id) DO UPDATE SET
               wins = wins + excluded.wins,
               losses = losses + excluded.losses,
               pushes = pushes + excluded.pushes,
               surrenders = surrenders + excluded.surrenders,
               blackjacks = blackjacks + excluded.blackjacks,
               total_wagered = total_wagered + excluded.total_wagered,
               total_won = total_won + excluded.total_won",
        )
        .bind(user_id.get() as i64)
        .bind(game.guild_id.get() as i64)
        .bind(if matches!(resolution[i].1, BjResolution::PlayerWin(_)) { 1 } else { 0 })
        .bind(if matches!(resolution[i].1, BjResolution::DealerWin(_)) { 1 } else { 0 })
        .bind(if matches!(resolution[i].1, BjResolution::Push) { 1 } else { 0 })
        .bind(if matches!(resolution[i].1, BjResolution::Surrender) { 1 } else { 0 })
        .bind(if matches!(resolution[i].1, BjResolution::PlayerWin(BjWinType::Blackjack)) { 1 } else { 0 })
        .bind(game.player_hands[i].wager)
        .bind(resolution[i].2)
        .execute(db)
        .await;
    }
}

pub async fn handle_bj_interaction(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let id = component.data.custom_id.clone();
    let id_parts: Vec<&str> = id.split('_').collect();
    let id_prefix = extract_prefix(id_parts);

    match id_prefix.as_str() {
        "bj_hit" => handle_hit(ctx, component, data).await,
        "bj_stand" => handle_stand(ctx, component, data).await,
        "bj_double" => handle_double(ctx, component, data).await,
        "bj_split" => handle_split(ctx, component, data).await,
        "bj_surrender" => handle_surrender(ctx, component, data).await,
        "bj_lobby_join" => handle_lobby_join(ctx, component, data).await,
        "bj_lobby_start" => handle_lobby_start(ctx, component, data).await,
        "bj_lobby_leave" => handle_lobby_leave(ctx, component, data).await,
        _ => println!("Unknown blackjack interaction: {}", id),
    }
}

async fn apply_action(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
    game_id: String,
    action: impl FnOnce(&mut BjGame),
) {
    let mut game_snapshot = {
        let mut games = data.bj_games.lock().await;
        let expired: Vec<_> = games
            .iter()
            .filter(|(_, g)| {
                g.current_phase != BjPhase::Lobby
                    && g.last_activity.elapsed().as_secs() >= GAME_TIMEOUT_SECS
            })
            .map(|(id, g)| {
                let refunds: Vec<(serenity::UserId, i64)> = g
                    .player_hands
                    .iter()
                    .enumerate()
                    .filter_map(|(i, ph)| {
                        if ph.resolution.is_none() && ph.state != HandState::Busted {
                            g.players.get(i).map(|(uid, _)| (*uid, ph.wager))
                        } else {
                            None
                        }
                    })
                    .collect();
                (id.clone(), g.guild_id, refunds)
            })
            .collect();

        for (id, guild_id, refunds) in expired {
            games.remove(&id);

            for (user_id, wager) in refunds {
                add_points(&data.db, user_id, guild_id, wager).await;
            }
        }

        match games.get(&game_id) {
            Some(g) => g.clone(),
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has expired or no longer exists.").await;
                return;
            }
        }
    };

    let user_id = component.user.id;
    if gate_checks(ctx, component, &game_snapshot, user_id)
        .await
        .is_err()
    {
        return;
    }

    action(&mut game_snapshot);

    let resolution = if game_snapshot.current_phase == BjPhase::DealerTurn {
        let mut deck = game_snapshot.deck;
        game_snapshot.dealer_turn(&mut deck);
        game_snapshot.deck = deck;

        let resolution = game_snapshot.resolve_game();
        apply_payouts(data, &mut game_snapshot, &resolution).await;
        Some(resolution)
    } else {
        None
    };

    let is_over = game_snapshot.current_phase == BjPhase::GameOver;
    let current_player = game_snapshot.players[game_snapshot.current].0;
    let balance = get_user_points(&data.db, current_player, game_snapshot.guild_id).await;

    let action_set = if is_over {
        actionset_disabled()
    } else {
        compute_available_actions(&game_snapshot, balance)
    };
    let content = game_snapshot.render_message();
    let row = vec![make_action_buttons(&game_id, action_set)];

    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .components(row),
            ),
        )
        .await
        .unwrap();

    if is_over {
        let resolution = resolution.expect("Resolution should be Some if game is over");
        data.bj_games.lock().await.remove(&game_id);
        update_bj_player_stats(data, &game_snapshot, resolution).await;
    } else {
        data.bj_games.lock().await.insert(game_id, game_snapshot);
    }
}

async fn handle_hit(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_hit_")
        .unwrap_or("");

    apply_action(ctx, component, data, game_id.into(), |g: &mut BjGame| {
        let mut deck = g.deck;
        g.apply_hit(&mut deck);
        g.deck = deck;
    })
    .await
}

async fn handle_stand(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_stand_")
        .unwrap_or("");

    apply_action(ctx, component, data, game_id.into(), |g: &mut BjGame| {
        g.apply_stand();
    })
    .await
}

async fn handle_double(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_double_")
        .unwrap_or("");

    let db = &data.db;
    let user_id = component.user.id;
    let guild_id = component.guild_id.unwrap_or_default();

    let wager = {
        let games = data.bj_games.lock().await;
        match games.get(game_id) {
            Some(g) => g.player_hands[g.current].wager,
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has expired or no longer exists.").await;
                return;
            }
        }
    };

    let balance = get_user_points(db, user_id, guild_id).await;
    if balance < wager {
        ephemeral_reply(
            ctx,
            component,
            "You don't have enough points to double down!",
        )
        .await;
        return;
    }

    deduct_points(db, user_id, guild_id, wager).await;
    apply_action(ctx, component, data, game_id.into(), |g| {
        let mut deck = g.deck;
        g.apply_double_down(&mut deck);
        g.deck = deck;
    })
    .await;
}

async fn handle_split(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_split_")
        .unwrap_or("");

    let db = &data.db;
    let user_id = component.user.id;
    let guild_id = component.guild_id.unwrap_or_default();

    let wager = {
        let games = data.bj_games.lock().await;
        match games.get(game_id) {
            Some(g) => g.player_hands[g.current].wager,
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has expired or no longer exists.").await;
                return;
            }
        }
    };

    let balance = get_user_points(db, user_id, guild_id).await;
    if balance < wager {
        ephemeral_reply(ctx, component, "You don't have enough points to split!").await;
        return;
    }

    deduct_points(db, user_id, guild_id, wager).await;
    apply_action(ctx, component, data, game_id.into(), |g| {
        let mut deck = g.deck;
        g.apply_split(&mut deck, wager);
        g.deck = deck;
    })
    .await;
}

async fn handle_surrender(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_surrender_")
        .unwrap_or("");

    apply_action(ctx, component, data, game_id.into(), |g: &mut BjGame| {
        g.apply_surrender();
    })
    .await;
}

async fn handle_lobby_join(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_lobby_join_")
        .unwrap_or("");
    let user_id = component.user.id;

    let game = {
        let mut games = data.bj_games.lock().await;
        let expired: Vec<String> = games
            .iter()
            .filter(|(_, g)| {
                g.current_phase == BjPhase::Lobby
                    && g.last_activity.elapsed().as_secs() > GAME_TIMEOUT_SECS
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired {
            let game = games.remove(&id).expect("Game should exist");
            for (uid, wager) in &game.players {
                add_points(&data.db, *uid, game.guild_id, *wager).await;
            }
        }

        match games.get(game_id).cloned() {
            Some(g) if g.current_phase == BjPhase::Lobby => g,
            Some(_) => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has already started.").await;
                return;
            }
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "Lobby not found.").await;
                return;
            }
        }
    };

    if game.players.iter().any(|(p, _)| *p == user_id) {
        ephemeral_reply(ctx, component, "You're already in this lobby!").await;
        return;
    }
    if game.is_lobby_full() {
        ephemeral_reply(ctx, component, "This lobby is already full!").await;
        return;
    }

    let modal_id = format!("bj_lobby_join_modal_{game_id}");
    let modal = create_bet_modal(modal_id);
    let _ = component.create_response(&ctx.http, modal).await;
}

pub async fn handle_lobby_join_modal(
    ctx: &serenity::Context,
    modal: &serenity::model::application::ModalInteraction,
    data: &GlobalTracker,
) {
    let modal_id = modal.data.custom_id.clone();
    let game_id = modal_id.strip_prefix("bj_lobby_join_modal_").unwrap_or("");
    let user_id = modal.user.id;

    let game = {
        let games = data.bj_games.lock().await;
        match games.get(game_id).cloned() {
            Some(g) if g.current_phase == BjPhase::Lobby => g,
            _ => {
                drop(games);
                modal_ephemeral_reply(ctx, modal, "Lobby no longer exists.").await;
                return;
            }
        }
    };

    if game.players.iter().any(|(p, _)| *p == user_id) {
        modal_ephemeral_reply(ctx, modal, "You're already in this lobby!").await;
        return;
    }
    if game.is_lobby_full() {
        modal_ephemeral_reply(ctx, modal, "This lobby is already full!").await;
        return;
    }

    let wager_str = extract_modal_value(modal, "wager_amount");
    let wager = match wager_str.and_then(|s| s.parse::<i64>().ok()) {
        Some(w) if w > 0 => w,
        _ => {
            modal_ephemeral_reply(
                ctx,
                modal,
                "Please enter a valid positive number for the wager.",
            )
            .await;
            return;
        }
    };

    let guild_id = game.guild_id;
    let balance = get_user_points(&data.db, user_id, guild_id).await;
    if balance < wager {
        modal_ephemeral_reply(
            ctx,
            modal,
            format!(
                "You don't have enough points to join with that wager! (Your balance: {balance} pts)"
            ).as_str(),
        ).await;
        return;
    }

    deduct_points(&data.db, user_id, guild_id, wager).await;

    let updated_game = {
        let mut bj_games = data.bj_games.lock().await;
        let game = match bj_games.get_mut(game_id) {
            Some(g) => g,
            None => {
                drop(bj_games);
                // Lobby expired between check and join — refund immediately
                add_points(&data.db, user_id, guild_id, wager).await;
                modal_ephemeral_reply(ctx, modal, "Lobby no longer exists.").await;
                return;
            }
        };
        game.players.push((user_id, wager));
        game.last_activity = Instant::now();
        game.clone()
    };

    let content = updated_game.render_lobby();
    let can_start = updated_game.players.len() >= 2;
    let row = make_lobby_buttons(game_id, false, can_start);
    let updated_msg = update_message(content, row);
    let _ = modal.create_response(&ctx.http, updated_msg).await;

    modal_ephemeral_reply(ctx, modal, "You've joined the lobby!").await;
}

async fn handle_lobby_start(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_lobby_start_")
        .unwrap_or("");
    let user_id = component.user.id;

    let game_snapshot = {
        let games = data.bj_games.lock().await;
        match games.get(game_id).cloned() {
            Some(g) if g.current_phase == BjPhase::Lobby => g,
            Some(_) => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has already started.").await;
                return;
            }
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "Lobby not found.").await;
                return;
            }
        }
    };

    if game_snapshot.host_id != user_id {
        ephemeral_reply(ctx, component, "Only the host can start the game.").await;
        return;
    }

    let started_game = {
        let mut games = data.bj_games.lock().await;
        let game = match games.get_mut(game_id) {
            Some(g) => g,
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "Lobby no longer exists.").await;
                return;
            }
        };
        game.start_game();
        game.clone()
    };

    if started_game.current_phase == BjPhase::DealerTurn {
        let mut game = started_game;
        let mut deck = game.deck;

        game.dealer_turn(&mut deck);
        game.deck = deck;

        let resolution = game.resolve_game();
        apply_payouts(data, &mut game, &resolution).await;

        let content = game.render_message();
        data.bj_games.lock().await.remove(game_id);
        update_bj_player_stats(data, &game, resolution).await;

        let _ = component
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content(content)
                        .components(vec![]),
                ),
            )
            .await;

        return;
    }

    let first_player_id = started_game.players[started_game.current].0;
    let balance = get_user_points(&data.db, first_player_id, started_game.guild_id).await;
    let action_set = compute_available_actions(&started_game, balance);
    let content = started_game.render_message();
    let row = make_action_buttons(&started_game.game_id, action_set);

    let updated_msg = update_message(content, row);
    let _ = component.create_response(&ctx.http, updated_msg).await;
}

async fn handle_lobby_leave(
    ctx: &serenity::Context,
    component: &ComponentInteraction,
    data: &GlobalTracker,
) {
    let game_id = component
        .data
        .custom_id
        .strip_prefix("bj_lobby_leave_")
        .unwrap_or("");
    let user_id = component.user.id;

    let game = {
        let games = data.bj_games.lock().await;
        match games.get(game_id).cloned() {
            Some(g) if g.current_phase == BjPhase::Lobby => g,
            Some(_) => {
                drop(games);
                ephemeral_reply(ctx, component, "This game has already started.").await;
                return;
            }
            None => {
                drop(games);
                ephemeral_reply(ctx, component, "Lobby not found.").await;
                return;
            }
        }
    };

    if !game.players.iter().any(|(p, _)| *p == user_id) {
        ephemeral_reply(ctx, component, "You are not in this lobby.").await;
        return;
    }

    let wager = game
        .players
        .iter()
        .find(|(p, _)| *p == user_id)
        .map(|(_, w)| *w)
        .unwrap_or(0);

    add_points(&data.db, user_id, game.guild_id, wager).await;

    if user_id == game.host_id {
        for (player_id, player_wager) in game.players.iter().filter(|(p, _)| *p != user_id) {
            add_points(&data.db, *player_id, game.guild_id, *player_wager).await;
        }

        data.bj_games.lock().await.remove(game_id);

        let _ = component.create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("The host has left the lobby. The lobby has been disbanded and wagers have been refunded.")
                    .components(vec![]),
            ),
        ).await;
    } else {
        let updated_game = {
            let mut bj_games = data.bj_games.lock().await;
            let game = match bj_games.get_mut(game_id) {
                Some(g) => g,
                None => {
                    drop(bj_games);
                    ephemeral_reply(ctx, component, "Lobby not found.").await;
                    return;
                }
            };
            game.players.retain(|(p, _)| *p != user_id);
            game.last_activity = Instant::now();
            game.clone()
        };

        let content = updated_game.render_lobby();
        let can_start = updated_game.players.len() >= 2;
        let row = make_lobby_buttons(game_id, false, can_start);
        let updated_msg = update_message(content, row);
        let _ = component.create_response(&ctx.http, updated_msg).await;

        ephemeral_reply(
            ctx,
            component,
            format!("You've left the lobby and your wager of {wager} pts has been refunded.")
                .as_str(),
        )
        .await;
    }
}
