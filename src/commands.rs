use rusqlite::params;
use crate::events::VoiceStateTracker;

// Type aliases for convenience
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, VoiceStateTracker, Error>;


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
            ctx.say("This command can only be used in a server!").await?;
            return Ok(());
        }
    };

    // Perform database operations in a separate scope to release lock before awaiting
    let leaderboard_data = ctx.data().get_leaderboard(guild_id, 10).await?;

    let mut leaderboard_text = String::from("🏆 **Voice Activity Leaderboard** 🏆\n\n");
    let mut rank = 1;

    for entry in leaderboard_data {
        leaderboard_text.push_str(&format!(
            "{}. <@{}> - {} points ({} minutes)\n",
            rank, entry.user_id, entry.total_points, entry.total_minutes
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
            ctx.say("This command can only be used in a server!").await?;
            return Ok(());
        }
    };

    // Perform database operations in a separate scope
    let (rank, points, minutes) = {
        let db = ctx.data().db.lock().await;
        
        // Get user's stats
        let user_stats: Option<(i32, i32)> = db.query_row(
            "SELECT total_points, total_minutes FROM users WHERE user_id = ?1 AND guild_id = ?2",
            params![user_id.get() as i64, guild_id.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?))
        ).ok();

        let (points, minutes) = match user_stats {
            Some(stats) => stats,
            None => {
                drop(db); // Release lock before awaiting
                ctx.say("You don't have any voice activity recorded yet! Join a voice channel to start earning points.").await?;
                return Ok(());
            }
        };

        // Get user's rank
        let rank: i32 = db.query_row(
            "SELECT COUNT(*) + 1 FROM users 
             WHERE guild_id = ?1 AND total_points > ?2",
            params![guild_id.get() as i64, points],
            |row| row.get(0)
        )?;

        (rank, points, minutes)
    }; // Lock is released here

    let response = format!(
        "📊 **Your Stats**\n\n\
         Rank: #{}\n\
         Total Points: {}\n\
         Total Time: {} minutes ({:.1} hours)",
        rank, points, minutes, minutes as f64 / 60.0
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
            ctx.say("This command can only be used in a server!").await?;
            return Ok(());
        }
    };

    // Perform database operations in a separate scope
    let (points, minutes, session_count, avg_duration, currently_active) = {
        let db = ctx.data().db.lock().await;
        
        // Get user's stats
        let user_stats: Option<(i32, i32)> = db.query_row(
            "SELECT total_points, total_minutes FROM users WHERE user_id = ?1 AND guild_id = ?2",
            params![user_id.get() as i64, guild_id.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?))
        ).ok();

        let (points, minutes) = match user_stats {
            Some(stats) => stats,
            None => {
                drop(db); // Release lock before awaiting
                ctx.say("You don't have any voice activity recorded yet! Join a voice channel to start earning points.").await?;
                return Ok(());
            }
        };

        // Get session count
        let session_count: i32 = db.query_row(
            "SELECT COUNT(*) FROM session_history WHERE user_id = ?1 AND guild_id = ?2",
            params![user_id.get() as i64, guild_id.get() as i64],
            |row| row.get(0)
        ).unwrap_or(0);

        // Get average session duration
        let avg_duration: f64 = db.query_row(
            "SELECT AVG(duration_minutes) FROM session_history WHERE user_id = ?1 AND guild_id = ?2",
            params![user_id.get() as i64, guild_id.get() as i64],
            |row| row.get(0)
        ).unwrap_or(0.0);

        drop(db); // Release database lock before locking active_sessions

        // Check if currently in voice
        let active_sessions = ctx.data().active_sessions.lock().await;
        let currently_active = active_sessions.contains_key(&(user_id, guild_id));

        (points, minutes, session_count, avg_duration, currently_active)
    }; // All locks are released here

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
        if currently_active { "✅ Yes" } else { "❌ No" }
    );

    ctx.say(response).await?;
    Ok(())
}