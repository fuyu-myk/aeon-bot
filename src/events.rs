use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::gateway::Ready;
use serenity::model::voice::VoiceState;
use serenity::model::id::{GuildId, ChannelId, UserId};
use poise::serenity_prelude as serenity;
use tokio::time::{interval, Duration};
use tokio::sync::Mutex;
use sqlx::{SqlitePool, Row};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::collections::HashMap;


pub struct VoiceStateTracker {
    pub db: SqlitePool,
    pub active_sessions: Arc<Mutex<HashMap<(UserId, GuildId), VoiceSession>>>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct VoiceSession {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub join_time: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct LeaderboardEntry {
    pub user_id: i64,
    pub username: String,
    pub total_points: i32,
    pub total_minutes: i32,
}

impl VoiceStateTracker {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let pool = SqlitePool::connect("sqlite:db/voice_logs.db").await?;
        
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                total_points INTEGER DEFAULT 0,
                total_minutes INTEGER DEFAULT 0,
                last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, guild_id)
            )"
        ).execute(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS active_sessions (
                user_id INTEGER,
                guild_id INTEGER,
                channel_id INTEGER,
                join_time TIMESTAMP,
                PRIMARY KEY (user_id, guild_id)
            )"
        ).execute(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS session_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                join_time TIMESTAMP NOT NULL,
                leave_time TIMESTAMP,
                duration_minutes INTEGER,
                points_awarded INTEGER
            )"
        ).execute(&pool).await?;

        Ok(Self {
            db: pool,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn handle_join_vc(&self, user_id: UserId, guild_id: GuildId, channel_id: ChannelId) {
        let join_time = Utc::now();
        let session = VoiceSession {
            user_id,
            guild_id,
            channel_id,
            join_time,
        };

        // Store in memory for quick access
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.insert((user_id, guild_id), session.clone());
        }

        // Store in db
        if let Err(e) = sqlx::query(
            "INSERT OR REPLACE INTO active_sessions
             (user_id, guild_id, channel_id, join_time) VALUES (?, ?, ?, ?)"
        )
        .bind(user_id.get() as i64)
        .bind(guild_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(session.join_time.timestamp())
        .execute(&self.db)
        .await {
            eprintln!("Database error on voice join: {}", e);
        }
    }

    async fn handle_leave_vc(&self, user_id: UserId, guild_id: GuildId, ctx: Context) {
        let session = {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&(user_id, guild_id))
        };

        if let Some(session) = session {
            let leave_time = Utc::now();
            let duration = leave_time - session.join_time;
            let duration_minutes = duration.num_minutes().max(0) as i32;
            let points_awarded = duration_minutes; // 1 point per minute

            let username = user_id.to_user(&ctx.http).await
                .map(|u| u.name)
                .unwrap_or_else(|_| "Unknown".to_string());

            // Update total points
            if let Err(e) = sqlx::query(
                "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
                 VALUES (?, ?, ?, 0, 0)"
            )
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .bind(&username)
            .execute(&self.db)
            .await {
                eprintln!("Error initializing user: {}", e);
            }

            if let Err(e) = sqlx::query(
                "UPDATE users SET
                 total_points = total_points + ?,
                 total_minutes = total_minutes + ?,
                 username = ?,
                 last_updated = CURRENT_TIMESTAMP
                 WHERE user_id = ? AND guild_id = ?"
            )
            .bind(points_awarded)
            .bind(duration_minutes)
            .bind(&username)
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .execute(&self.db)
            .await {
                eprintln!("Error updating user points: {}", e);
            }

            // Log session history
            if let Err(e) = sqlx::query(
                "INSERT INTO session_history
                 (user_id, guild_id, channel_id, join_time, leave_time, duration_minutes, points_awarded)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .bind(session.channel_id.get() as i64)
            .bind(session.join_time.timestamp())
            .bind(leave_time.timestamp())
            .bind(duration_minutes)
            .bind(points_awarded)
            .execute(&self.db)
            .await {
                eprintln!("Error logging session history: {}", e);
            }

            // Remove from active_sessions
            if let Err(e) = sqlx::query(
                "DELETE FROM active_sessions WHERE user_id = ? AND guild_id = ?"
            )
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .execute(&self.db)
            .await {
                eprintln!("Error removing active session: {}", e);
            }
        }
    }

    async fn award_points(
        db: &SqlitePool,
        active_sessions: &Arc<Mutex<HashMap<(UserId, GuildId), VoiceSession>>>,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sessions = active_sessions.lock().await;

        for ((user_id, guild_id), _session) in sessions.iter() {
            let username = user_id.to_user(&ctx.http).await
                .map(|u| u.name)
                .unwrap_or_else(|_| "Unknown".to_string());

            sqlx::query(
                "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
                 VALUES (?, ?, ?, 0, 0)"
            )
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .bind(&username)
            .execute(db)
            .await?;

            sqlx::query(
                "UPDATE users SET
                 total_points = total_points + 1,
                 total_minutes = total_minutes + 1,
                 username = ?,
                 last_updated = CURRENT_TIMESTAMP
                 WHERE user_id = ? AND guild_id = ?"
            )
            .bind(&username)
            .bind(user_id.get() as i64)
            .bind(guild_id.get() as i64)
            .execute(db)
            .await?;
        }

        Ok(())
    }

    pub async fn get_leaderboard(&self, guild_id: GuildId, limit: i32) -> Result<Vec<LeaderboardEntry>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT user_id, username, total_points, total_minutes 
             FROM users 
             WHERE guild_id = ? AND total_points > 0
             ORDER BY total_points DESC 
             LIMIT ?"
        )
        .bind(guild_id.get() as i64)
        .bind(limit)
        .fetch_all(&self.db)
        .await?;

        let entries = rows.into_iter().map(|row| {
            LeaderboardEntry {
                user_id: row.get("user_id"),
                username: row.get("username"),
                total_points: row.get("total_points"),
                total_minutes: row.get("total_minutes"),
            }
        }).collect();

        Ok(entries)
    }
}

#[async_trait]
impl EventHandler for VoiceStateTracker {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        // Start point awarding timer
        let db = self.db.clone();
        let active_sessions = Arc::clone(&self.active_sessions);

        tokio::spawn(
            async move {
                let mut interval = interval(Duration::from_secs(60));
                interval.tick().await;

                loop {
                    interval.tick().await;

                    if let Err(e) = Self::award_points(&db, &active_sessions, &ctx).await {
                        eprintln!("Error awarding points: {}", e);
                    }
                }
            }
        );
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let user_id = new.user_id;
        let guild_id = match new.guild_id {
            Some(id) => id,
            None => return, // Ignore DM voice states
        };

        // Voice state changes
        match (old.as_ref().and_then(|vs| vs.channel_id), new.channel_id) {
            // User joined vc
            (None, Some(channel_id)) | (Some(_), Some(channel_id)) if old.as_ref().map(|vs| vs.channel_id) != Some(Some(channel_id))=> {
                self.handle_join_vc(user_id, guild_id, channel_id).await;
            },
            // User left vc
            (Some(_), None) => {
                self.handle_leave_vc(user_id, guild_id, ctx).await;
            },
            // User switched vc
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                self.handle_leave_vc(user_id, guild_id, ctx).await;
                self.handle_join_vc(user_id, guild_id, new_channel).await;
            }
            _ => {}
        }
    }
}