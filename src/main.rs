mod events;
mod commands;

use poise::serenity_prelude as serenity;
use events::VoiceStateTracker;
use commands::{ping, leaderboard, rank, stats};
use std::sync::Arc;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Load Discord token from environment
    let token = std::env::var("DISCORD_TOKEN")
        .expect("Expected DISCORD_TOKEN in environment");

    // Set up gateway intents
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_VOICE_STATES
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    // Create the voice state tracker
    let tracker = VoiceStateTracker::new()
        .await
        .expect("Failed to initialize database");

    let db = Arc::clone(&tracker.db);
    let active_sessions = Arc::clone(&tracker.active_sessions);

    // Build the poise framework
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                ping(),
                leaderboard(),
                rank(),
                stats(),
            ],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                
                Ok(VoiceStateTracker {
                    db,
                    active_sessions,
                })
            })
        })
        .build();

    // Create the Discord client
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .event_handler(tracker)
        .await
        .expect("Error creating client");

    // Start the bot
    println!("Starting aeon-bot...");
    if let Err(why) = client.start().await {
        eprintln!("Client error: {:?}", why);
    }

    Ok(())
}