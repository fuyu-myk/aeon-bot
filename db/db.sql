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

-- Tic-Tac-Toe: Q-learning value table
-- Stores the bot's learned policy across all board states.
-- state_key is a 9-char string: '_', 'X', or 'O' per cell (row-major).
-- action is the cell index 0–8 the bot played from that state.
CREATE TABLE IF NOT EXISTS ttt_q_table (
    state_key   TEXT    NOT NULL,
    action      INTEGER NOT NULL,
    q_value     REAL    DEFAULT 0.0,
    visit_count INTEGER DEFAULT 0,
    PRIMARY KEY (state_key, action)
);

-- Tic-Tac-Toe: bot aggregate win/loss/draw record (singleton, id = 1)
CREATE TABLE IF NOT EXISTS ttt_bot_stats (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    total_games INTEGER DEFAULT 0,
    wins        INTEGER DEFAULT 0,
    losses      INTEGER DEFAULT 0,
    draws       INTEGER DEFAULT 0
);

-- Tic-Tac-Toe: per-player stats within each guild
CREATE TABLE IF NOT EXISTS ttt_player_stats (
    user_id  INTEGER NOT NULL,
    guild_id INTEGER NOT NULL,
    wins     INTEGER DEFAULT 0,
    losses   INTEGER DEFAULT 0,
    draws    INTEGER DEFAULT 0,
    PRIMARY KEY (user_id, guild_id)
);

-- Blackjack: per-player stats within each guild
CREATE TABLE IF NOT EXISTS bj_player_stats (
    user_id       INTEGER NOT NULL,
    guild_id      INTEGER NOT NULL,
    wins          INTEGER NOT NULL DEFAULT 0,
    losses        INTEGER NOT NULL DEFAULT 0,
    pushes        INTEGER NOT NULL DEFAULT 0,
    surrenders    INTEGER NOT NULL DEFAULT 0,
    blackjacks    INTEGER NOT NULL DEFAULT 0,
    total_wagered INTEGER NOT NULL DEFAULT 0,
    total_won     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, guild_id)
);
