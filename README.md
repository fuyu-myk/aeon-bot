# AeonBot

A Discord bot (that's born out of boredom) built with Rust that tracks voice channel activity and awards points to users based on their time spent in voice channels.

## Features

- **Voice Activity Tracking**: Automatically tracks when users join and leave voice channels
- **Points System**: Awards 1 point per minute spent in voice channels
- **Leaderboard**: View top users by voice activity
- **Statistics**: View detailed stats about your voice activity

## Commands

More info can be found under [`COMMANDS.md`](docs/COMMANDS.md). A summary of the commands are as follows:

- `/ping` - Check if the bot is responsive
- `/leaderboard` - Display the voice activity leaderboard (top 10 users)
- `/rank` - View your current rank and stats
- `/stats` - View detailed voice activity statistics

## Running the project

Clone this repository and run `start.sh`. It will set up the necessary database files, provide instructions to set up the `.env` file, build and run the bot.