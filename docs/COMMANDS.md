# Aeon Bot - Quick Reference

## Commands

### `/ping`

Check if the bot is responsive.

- **Usage**: `/ping`
- **Response**: Pong!

### `/leaderboard`

Display the top 10 users by voice activity.

- **Usage**: `/leaderboard`
- **Shows**:
  - User rank
  - Total points
  - Total minutes in voice
- **Scope**: Server-specific

### `/rank`

View your current rank and basic stats.

- **Usage**: `/rank`
- **Shows**:
  - Your rank in the server
  - Total points earned
  - Total time in voice (minutes and hours)
- **Scope**: Server-specific

### `/stats`

View detailed voice activity statistics.

- **Usage**: `/stats`
- **Shows**:
  - Total points
  - Total time (minutes and hours)
  - Number of voice sessions
  - Average session duration
  - Current voice status
- **Scope**: Server-specific

### `/ttt`

Play a game of Tic Tac Toe.

- **Usage**: `/ttt [subcommand]`
  - `play`: Start a new game against the bot
  - `challenge`: Challenge another user to a game with an optional wager
  - `stats`: View bot's win/loss/draw record

### `/bj`

Play a game of Blackjack.

- **Usage**: `/bj [subcommand]`
  - `play`: Start a new solo game with wagers
  - `host`: Host a game for other users to join with wagers
  - `stats`: View user's detailed win/loss record

## How Points Work

- **1 point per minute** in any voice channel
- Points awarded every 60 seconds while in voice
- Points calculated when leaving voice channel
- Works across all voice channels in a server
- Each server has its own separate leaderboard

## Technical Details

- Commands can be used as slash commands (`/command`) or prefix commands (`~command`)
- All data is stored per-server (guild)
- Database is SQLite, stored in `db/voice_logs.db`
- Bot requires `GUILDS`, `GUILD_VOICE_STATES`, and `MESSAGE_CONTENT` intents

## Troubleshooting

**Commands not showing up?**

- Make sure bot has `applications.commands` scope when invited
- Wait a few seconds after bot starts (commands register on startup)
- Try refreshing Discord (Ctrl+R / Cmd+R)

**Not earning points?**

- Make sure bot has permission to see voice states
- Check that you're in a voice channel (not AFK channel)
- Points update every minute, not instantly

**Stats showing 0?**

- You need to join and leave a voice channel first
- Stay in voice for at least 1 minute to earn points

## Bot Permissions Required

Minimum permissions:

- View Channels
- Send Messages  
- Connect (to see voice states)
- Read Message History
- Use Slash Commands

Recommended permission integer: `2148600832`
