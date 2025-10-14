-- Users table to store point totals
CREATE TABLE IF NOT EXISTS users (
    user_id INTEGER NOT NULL,
    guild_id INTEGER NOT NULL,
    username TEXT NOT NULL,
    total_points INTEGER DEFAULT 0,
    total_minutes INTEGER DEFAULT 0,
    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, guild_id)
);

-- Active sessions (tracking voice chat activity)
CREATE TABLE IF NOT EXISTS active_sessions (
    user_id INTEGER,
    guild_id INTEGER,
    channel_id INTEGER,
    join_time TIMESTAMP,
    PRIMARY KEY (user_id, guild_id)
);

-- Session history (for logging)
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