# AeonBot

A Discord bot (that's born out of boredom) built with Rust that tracks voice channel activity, awards points to users, and hosts self-learning Tic-Tac-Toe games.

## Features

- **Voice Activity Tracking**: Automatically tracks when users join and leave voice channels
- **Points System**: Awards 1 point per minute spent in voice channels
- **Leaderboard**: View top users by voice activity
- **Statistics**: View detailed stats about your voice activity
- **Self-Learning Tic-Tac-Toe**: Play against a bot that improves over time via Q-learning / TD learning
- **PvP Tic-Tac-Toe**: Challenge other users to games with point wagers; observers can bet on outcomes

## Commands

More info can be found under [`COMMANDS.md`](docs/COMMANDS.md). A summary of the commands are as follows:

### Voice Activity

- `/ping` — Check if the bot is responsive
- `/leaderboard` — Display the voice activity leaderboard (top 10)
- `/rank` — View your current rank and point total
- `/stats` — View detailed voice activity statistics

### Tic-Tac-Toe

- `/ttt` — Show Tic-Tac-Toe help
- `/ttt play` — Play a game against the self-learning bot
- `/ttt stats` — View the bot's all-time win/loss/draw record and current experience tier
- `/ttt challenge @user [wager]` — Challenge another user; optionally wager points (winner takes both stakes)

### Blackjack

- `/bj` — Show Blackjack help
- `/bj play` — Play a game of Blackjack against the bot (dealer)
- `/bj host` — Host a multiplayer Blackjack game (other players can join)
- `/bj stats` — View your Blackjack game stats (total games, wins, losses, etc.)

## Tic-Tac-Toe — How It Works

### Self-learning bot

The bot learns using **Q-learning** (a model-free reinforcement learning algorithm) with **Temporal Difference (TD-0) updates** applied over each completed game. Its policy is persisted in a SQLite table so it carries knowledge across bot restarts.

The bot's move quality improves the more games it plays:

| Experience tier | Games played | Bot strength | Exploration rate (ε) |
| --- | --- | --- | --- |
| 🐣 Novice | 0–49 | Very beatable | ~83% random moves |
| 🔰 Apprentice | 50–199 | Learning | ~35% random moves |
| ⚔️ Veteran | 200–499 | Competent | ~10% random moves |
| 💀 Expert | 500+ | Near-optimal | ~5% random moves |

### Point rewards for beating the bot

Rewards scale by how experienced the bot is when you beat it — harder victories pay more:

| Bot tier at game start | Reward for winning |
| --- | --- |
| 🐣 Novice (0–49 games) | **+5 pts** |
| 🔰 Apprentice (50–199 games) | **+10 pts** |
| ⚔️ Veteran (200–499 games) | **+15 pts** |
| 💀 Expert (500+ games) | **+25 pts** |

A draw earns +1 pt. A loss awards nothing.

### PvP Challenges

Use `/ttt challenge @user` to start a game against another player. Both players interact with the same board message by clicking the cells on their turn.

- **Wagers**: add `[wager]` to stake points. Both players' stakes are held; the winner receives both (or each gets a refund on a draw).
- **Observer betting**: while a PvP game is in progress, anyone watching can bet 10 points on ❌, ⭕, or Draw. Correct bets return 2× (net +10 pts); incorrect bets are forfeited (net -10 pts).

## Running the project

Clone this repository and run `start.sh`. It will set up the necessary database files, provide instructions to set up the `.env` file, build and run the bot.
