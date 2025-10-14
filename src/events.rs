use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::gateway::Ready;
use serenity::model::voice::VoiceState;
use serenity::model::id::{GuildId, ChannelId, UserId};
use poise::serenity_prelude as serenity;
use tokio::time::{interval, Duration};
use tokio::sync::Mutex;
use rusqlite::{params, Connection};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::collections::HashMap;


pub struct VoiceStateTracker {
    pub db: Arc<Mutex<Connection>>,
    pub active_sessions: Arc<Mutex<HashMap<(UserId, GuildId), VoiceSession>>>,
}

#[derive(Clone)]
pub struct VoiceSession {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub join_time: DateTime<Utc>,
}

#[derive(Debug)]
pub struct LeaderboardEntry {
    pub user_id: i64,
    pub username: String,
    pub total_points: i32,
    pub total_minutes: i32,
}

impl VoiceStateTracker {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open("db/voice_logs.db")?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS users (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                total_points INTEGER DEFAULT 0,
                total_minutes INTEGER DEFAULT 0,
                last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, guild_id)
            );

            CREATE TABLE IF NOT EXISTS active_sessions (
                user_id INTEGER,
                guild_id INTEGER,
                channel_id INTEGER,
                join_time TIMESTAMP,
                PRIMARY KEY (user_id, guild_id)
            );

            CREATE TABLE IF NOT EXISTS session_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                join_time TIMESTAMP NOT NULL,
                leave_time TIMESTAMP,
                duration_minutes INTEGER,
                points_awarded INTEGER
            );
        ")?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
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
        let db = self.db.lock().await;
        if let Err(e) = db.execute(
            "INSERT OR REPLACE INTO active_sessions
             (user_id, guild_id, channel_id, join_time) VALUES (?1, ?2, ?3, ?4)",
            params![user_id.get() as i64, guild_id.get() as i64, channel_id.get() as i64, session.join_time.timestamp()]
        ) {
            eprintln!("Database error on voice join: {}", e);
        }
    }

    async fn handle_leave_vc(&self, user_id: UserId, guild_id: GuildId) {
        let session = {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&(user_id, guild_id))
        };

        if let Some(session) = session {
            let leave_time = Utc::now();
            let duration = leave_time - session.join_time;
            let duration_minutes = duration.num_minutes().max(0) as i32;
            let points_awarded = duration_minutes; // 1 point per minute

            let db = self.db.lock().await;

            // Update total points
            if let Err(e) = db.execute(
                "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
                 VALUES (?1, ?2, 'Unknown', 0, 0)",
                params![user_id.get() as i64, guild_id.get() as i64]
            ) {
                eprintln!("Error initializing user: {}", e);
            }

            if let Err(e) = db.execute(
                "UPDATE users SET
                 total_points = total_points + ?1,
                 total_minutes = total_minutes + ?2,
                 last_updated = CURRENT_TIMESTAMP
                 WHERE user_id = ?3 AND guild_id = ?4",
                params![points_awarded, duration_minutes, user_id.get() as i64, guild_id.get() as i64],
            ) {
                eprintln!("Error updating user points: {}", e);
            }

            // Log session history
            if let Err(e) = db.execute(
                "INSERT INTO session_history
                 (user_id, guild_id, channel_id, join_time, leave_time, duration_minutes, points_awarded)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    user_id.get() as i64,
                    guild_id.get() as i64,
                    session.channel_id.get() as i64,
                    session.join_time.timestamp(),
                    leave_time.timestamp(),
                    duration_minutes,
                    points_awarded
                ]
            ) {
                eprintln!("Error logging session history: {}", e);
            }

            // Remove from active_sessions
            if let Err(e) = db.execute(
                "DELETE FROM active_sessions WHERE user_id = ?1 AND guild_id = ?2",
                params![user_id.get() as i64, guild_id.get() as i64]
            ) {
                eprintln!("Error removing active session: {}", e);
            }
        }
    }

    async fn award_points(
        db: &Arc<Mutex<Connection>>,
        active_sessions: &Arc<Mutex<HashMap<(UserId, GuildId), VoiceSession>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sessions = active_sessions.lock().await;
        let db = db.lock().await;

        for ((user_id, guild_id), _session) in sessions.iter() {
            db.execute(
                "INSERT OR IGNORE INTO users (user_id, guild_id, username, total_points, total_minutes)
                 VALUES (?1, ?2, 'Unknown', 0, 0)",
                params![user_id.get() as i64, guild_id.get() as i64]
            )?;

            db.execute(
                "UPDATE users SET
                 total_points = total_points + 1,
                 total_minutes = total_minutes + 1,
                 last_updated = CURRENT_TIMESTAMP
                 WHERE user_id = ?1 AND guild_id = ?2",
                params![user_id.get() as i64, guild_id.get() as i64],
            )?;
        }

        Ok(())
    }

    pub async fn get_leaderboard(&self, guild_id: GuildId, limit: i32) -> Result<Vec<LeaderboardEntry>, rusqlite::Error> {
        let db = self.db.lock().await;
        
        let mut stmt = db.prepare(
            "SELECT user_id, username, total_points, total_minutes 
             FROM users 
             WHERE guild_id = ?1 AND total_points > 0
             ORDER BY total_points DESC 
             LIMIT ?2"
        )?;

        let rows = stmt.query_map(params![guild_id.get() as i64, limit], |row| {
            Ok(LeaderboardEntry {
                user_id: row.get::<_, i64>(0)?,
                username: row.get::<_, String>(1)?,
                total_points: row.get::<_, i32>(2)?,
                total_minutes: row.get::<_, i32>(3)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        Ok(entries)
    }
}

#[async_trait]
impl EventHandler for VoiceStateTracker {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        // Start point awarding timer
        let db = Arc::clone(&self.db);
        let active_sessions = Arc::clone(&self.active_sessions);

        tokio::spawn(
            async move {
                let mut interval = interval(Duration::from_secs(60));
                interval.tick().await;

                loop {
                    interval.tick().await;

                    if let Err(e) = Self::award_points(&db, &active_sessions).await {
                        eprintln!("Error awarding points: {}", e);
                    }
                }
            }
        );
    }

    async fn voice_state_update(&self, _ctx: Context, old: Option<VoiceState>, new: VoiceState) {
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
                self.handle_leave_vc(user_id, guild_id).await;
            },
            // User switched vc
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                self.handle_leave_vc(user_id, guild_id).await;
                self.handle_join_vc(user_id, guild_id, new_channel).await;
            }
            _ => {}
        }
    }
}