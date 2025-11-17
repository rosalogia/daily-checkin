use anyhow::Result;
use serenity::prelude::*;
use tracing::{info, warn, error};

mod data;
mod bot;
mod handler;
mod commands;
mod utils;
mod scheduler;
mod streaks;

use data::BotData;
use bot::Bot;
use handler::Handler;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    
    // Initialize logging with environment-based configuration
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "daily_checkin_bot=info,serenity=warn".to_string())
        )
        .init();

    info!("Starting Daily Check-in Bot...");

    // Load bot data
    let bot_data = match BotData::load().await {
        Ok(data) => {
            info!("Successfully loaded bot data");
            data
        },
        Err(e) => {
            warn!("Failed to load bot data, starting fresh: {}", e);
            BotData::default()
        }
    };

    let bot = Bot::new(bot_data);

    // Validate environment variables
    let token = std::env::var("DISCORD_TOKEN")
        .map_err(|_| anyhow::anyhow!("DISCORD_TOKEN environment variable is required"))?;

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler {
        data: bot.data.clone(),
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Discord client: {}", e))?;

    info!("Bot initialized successfully, connecting to Discord...");

    // Start the daily scheduler in the background
    // We'll start it from the Ready event handler instead since we need a proper Context
    info!("Daily scheduler will start when bot is ready");

    // Set up graceful shutdown
    if let Err(why) = client.start().await {
        error!("Discord client error: {}", why);
        return Err(anyhow::anyhow!("Discord client failed: {}", why));
    }

    Ok(())
}