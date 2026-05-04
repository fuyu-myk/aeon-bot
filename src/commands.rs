use crate::events::GlobalTracker;
use sqlx::Row;

// Type aliases for convenience
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, GlobalTracker, Error>;

/// Check if the bot is responsive
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}

/// Display the voice activity leaderboard
#[poise::command(slash_command, prefix_command)]
pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };

    // Perform database operations in a separate scope to release lock before awaiting
    let leaderboard_data = ctx.data().get_leaderboard(guild_id, 10).await?;

    let mut leaderboard_text = String::from("🏆 **Voice Activity Leaderboard** 🏆\n\n");
    let mut rank = 1;

    for entry in leaderboard_data {
        leaderboard_text.push_str(&format!(
            "{}. {} - {} points ({} minutes)\n",
            rank, entry.username, entry.total_points, entry.total_minutes
        ));
        rank += 1;
    }

    if rank == 1 {
        leaderboard_text.push_str("No data yet! Join a voice channel to start earning points.");
    }

    ctx.say(leaderboard_text).await?;
    Ok(())
}

/// View your current rank and stats
#[poise::command(slash_command, prefix_command)]
pub async fn rank(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("This command can only be used in a server!")
                .await?;
            return Ok(());
        }
    };

    // Perform database operations
    let user_stats = sqlx::query(
        "SELECT total_points, total_minutes FROM users WHERE user_id = ? AND guild_id = ?",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .fetch_optional(&ctx.data().db)
    .await?;

    let (points, minutes) = match user_stats {
        Some(row) => {
            let points: i32 = row.get("total_points");
            let minutes: i32 = row.get("total_minutes");
            (points, minutes)
        }
        None => {
            ctx.say("You don't have any voice activity recorded yet! Join a voice channel to start earning points.").await?;
            return Ok(());
        }
    };

    // Get user's rank
    let rank_row = sqlx::query(
        "SELECT COUNT(*) + 1 as rank FROM users 
         WHERE guild_id = ? AND total_points > ?",
    )
    .bind(guild_id.get() as i64)
    .bind(points)
    .fetch_one(&ctx.data().db)
    .await?;

    let rank: i32 = rank_row.get("rank");

    let response = format!(
        "📊 **Your Stats**\n\n\
         Rank: #{}\n\
         Total Points: {}\n\
         Total Time: {} minutes ({:.1} hours)",
        rank,
        points,
        minutes,
        minutes as f64 / 60.0,
    );

    ctx.say(response).await?;
    Ok(())
}

/// View detailed voice activity statistics
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

    // Perform database operations
    let user_stats = sqlx::query(
        "SELECT total_points, total_minutes FROM users WHERE user_id = ? AND guild_id = ?",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .fetch_optional(&ctx.data().db)
    .await?;

    let (points, minutes) = match user_stats {
        Some(row) => {
            let points: i32 = row.get("total_points");
            let minutes: i32 = row.get("total_minutes");
            (points, minutes)
        }
        None => {
            ctx.say("You don't have any voice activity recorded yet! Join a voice channel to start earning points.").await?;
            return Ok(());
        }
    };

    // Get session count
    let session_count_row = sqlx::query(
        "SELECT COUNT(*) as count FROM session_history WHERE user_id = ? AND guild_id = ?",
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .fetch_one(&ctx.data().db)
    .await?;
    let session_count: i32 = session_count_row.get("count");

    // Get average session duration
    let avg_duration_row = sqlx::query(
        "SELECT AVG(duration_minutes) as avg FROM session_history WHERE user_id = ? AND guild_id = ?"
    )
    .bind(user_id.get() as i64)
    .bind(guild_id.get() as i64)
    .fetch_one(&ctx.data().db)
    .await?;
    let avg_duration: Option<f64> = avg_duration_row.get("avg");
    let avg_duration = avg_duration.unwrap_or(0.0);

    // Check if currently in voice
    let active_sessions = ctx.data().active_sessions.lock().await;
    let currently_active = active_sessions.contains_key(&(user_id, guild_id));

    let response = format!(
        "📈 **Detailed Voice Stats**\n\n\
         Total Points: {}\n\
         Total Time: {} minutes ({:.1} hours)\n\
         Session Count: {}\n\
         Average Session: {:.1} minutes\n\
         Currently Active: {}",
        points,
        minutes,
        minutes as f64 / 60.0,
        session_count,
        avg_duration,
        if currently_active {
            "✅ Yes"
        } else {
            "❌ No"
        },
    );

    ctx.say(response).await?;
    Ok(())
}
