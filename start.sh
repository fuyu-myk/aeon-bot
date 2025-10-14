#!/bin/bash
# Quick start script for Aeon Bot

set -e

echo "Aeon Bot Setup & Start"
echo "======================"
echo ""

# Check if .env exists
if [ ! -f .env ]; then
    echo ".env file not found!"
    echo "Creating .env from .env.example..."
    cp .env.example .env
    echo ""
    echo "Please edit .env and add your Discord bot token"
    echo "Then run this script again."
    exit 1
fi

# Check if DISCORD_TOKEN is set
source .env
if [ -z "$DISCORD_TOKEN" ] || [ "$DISCORD_TOKEN" = "your_discord_token_here" ]; then
    echo "DISCORD_TOKEN not configured in .env"
    echo "Please edit .env and add your Discord bot token"
    exit 1
fi

# Create database directory if it doesn't exist
if [ ! -d db ]; then
    echo "Creating database directory..."
    mkdir db
fi

# Build the bot
echo "Building bot..."
cargo build --release

# Run the bot
echo ""
echo "Starting bot..."
echo ""
cargo run --release