use serenity::all::MessageId;
use sqlx::Row;

use crate::{
    events::GlobalTracker,
    games::{
        bj::{
            BjGame, BjPhase, compute_available_actions,
            interactions::{apply_payouts, update_bj_player_stats},
            make_action_buttons, make_lobby_buttons,
        },
        lib::{deduct_points, generate_game_id, get_user_points},
    },
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, GlobalTracker, Error>;

/// Blackjack – play against the bot (dealer) or play against
/// other players, or check stats
#[poise::command(slash_command, prefix_command, subcommands("play", "host", "stats"))]
pub async fn bj(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "**Blackjack commands:**\n\
         • `/bj play` — start a new game\n\
         • `/bj host` — host a new game\n\
         • `/bj stats` — view your blackjack stats",
    )
    .await?;

    Ok(())
}

/// Start a new game of blackjack against the bot (dealer) or other players
#[poise::command(slash_command, prefix_command)]
pub async fn play(ctx: Context<'_>, wager: i64) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };
    let channel_id = ctx.channel_id();

    if wager <= 0 {
        ctx.say("Wager must be a positive amount! >.>").await?;
        return Ok(());
    }

    let db = &ctx.data().db;
    let balance = get_user_points(db, user_id, guild_id).await;

    if wager > balance {
        ctx.say(format!(
            "You don't have enough points to place that wager! (Your balance: {balance} pts)"
        ))
        .await?;
        return Ok(());
    }

    {
        let games = ctx.data().bj_games.lock().await;
        let already_playing = games
            .values()
            .any(|g| g.guild_id == guild_id && g.players.iter().any(|(p, _)| *p == user_id));

        if already_playing {
            ctx.say("You already have an active blackjack game in this server! Finish it first.")
                .await?;
            return Ok(());
        }
    }

    deduct_points(db, user_id, guild_id, wager).await;

    let game_id = generate_game_id();
    let mut game = BjGame::new(
        game_id.clone(),
        guild_id,
        channel_id,
        MessageId::new(1), // placeholder, will be updated after sending the initial message
        user_id,
        wager,
    );

    if game.current_phase == BjPhase::DealerTurn {
        let mut deck = game.deck;

        game.dealer_turn(&mut deck);
        game.deck = deck;

        let resolution = game.resolve_game();
        apply_payouts(ctx.data(), &mut game, &resolution).await;

        let content = game.render_message();
        update_bj_player_stats(ctx.data(), &game, resolution).await;

        ctx.send(poise::CreateReply::default().content(content))
            .await?;
        return Ok(());
    }

    let action_set = compute_available_actions(&game, balance);
    let content = game.render_message();
    let row = make_action_buttons(&game_id, action_set);

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(content)
                .components(vec![row]),
        )
        .await?;
    let message = reply.message().await?;
    game.message_id = message.id;

    ctx.data().bj_games.lock().await.insert(game_id, game);

    Ok(())
}

/// Host a new game of blackjack and wait for other players to join before starting the game
#[poise::command(slash_command, prefix_command)]
pub async fn host(ctx: Context<'_>, wager: i64) -> Result<(), Error> {
    let host_id = ctx.author().id;
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };
    let channel_id = ctx.channel_id();

    if wager <= 0 {
        ctx.say("Wager must be a positive amount! >.>").await?;
        return Ok(());
    }

    let db = &ctx.data().db;
    let balance = get_user_points(db, host_id, guild_id).await;

    if wager > balance {
        ctx.say(format!(
            "You don't have enough points to place that wager! (Your balance: {balance} pts)"
        ))
        .await?;
        return Ok(());
    }

    {
        let games = ctx.data().bj_games.lock().await;
        let already_in_game = games
            .values()
            .any(|g| g.guild_id == guild_id && g.players.iter().any(|(p, _)| *p == host_id));
        let already_hosting = games.values().any(|g| {
            g.current_phase == BjPhase::Lobby
                && g.guild_id == guild_id
                && g.channel_id == channel_id
                && g.host_id == host_id
        });

        if already_in_game {
            ctx.say("You already have an active blackjack game in this server! Finish it first.")
                .await?;
            return Ok(());
        }
        if already_hosting {
            ctx.say("You are already hosting a blackjack game in this channel! Wait for players to join or cancel it first.").await?;
            return Ok(());
        }
    }

    deduct_points(db, host_id, guild_id, wager).await;
    let game_id = generate_game_id();
    let mut game = BjGame::new_lobby(
        game_id.clone(),
        guild_id,
        channel_id,
        MessageId::new(1), // placeholder, will be updated after sending the initial message
        host_id,
        wager,
    );

    let content = game.render_lobby();
    let row = make_lobby_buttons(&game_id, true, false);
    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(content)
                .components(vec![row]),
        )
        .await?;
    let message = reply.message().await?;
    game.message_id = message.id;

    ctx.data().bj_games.lock().await.insert(game_id, game);

    Ok(())
}

/// View your blackjack stats (total games, wins, losses, etc.)
#[poise::command(slash_command, prefix_command)]
pub async fn stats(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };
    let db = &ctx.data().db;

    let row = sqlx::query("SELECT * FROM bj_player_stats WHERE user_id = ? AND guild_id = ?")
        .bind(user_id.get() as i64)
        .bind(guild_id.get() as i64)
        .fetch_optional(db)
        .await?;

    let stats_msg = match row {
        Some(r) => {
            let wins = r.get::<i64, _>("wins");
            let losses = r.get::<i64, _>("losses");
            let pushes = r.get::<i64, _>("pushes");
            let surrenders = r.get::<i64, _>("surrenders");
            let blackjacks = r.get::<i64, _>("blackjacks");
            let total_wagered = r.get::<i64, _>("total_wagered");
            let total_won = r.get::<i64, _>("total_won");

            let total = wins + losses + pushes + surrenders;
            let win_rate = if total > 0 {
                wins as f64 / total as f64 * 100.0
            } else {
                0.0
            };

            format!(
                "**<@{user_id}>'s Blackjack Stats**\n\
                 Total games:   {total}\n\
                 Wins: {wins}  Losses: {losses}  Pushes: {pushes}\n\
                 Surrenders:    {surrenders}\n\
                 Blackjacks:    {blackjacks}\n\
                 Win Rate:      {win_rate:.2}%\n\
                 Total Wagered: {total_wagered}\n\
                 Total Won:     {total_won}",
            )
        }
        None => "You don't have any blackjack stats yet!".into(),
    };

    ctx.say(stats_msg).await?;

    Ok(())
}
