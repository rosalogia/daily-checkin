use anyhow::Result;
use serenity::{
    async_trait,
    builder::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage},
    model::{
        application::{Command, Interaction},
        gateway::Ready,
    },
    prelude::*,
};
use tracing::{error, info, warn, debug};

mod data;

use data::BotData;
use std::sync::Arc;
use tokio::sync::RwLock;

struct Handler {
    data: Arc<RwLock<BotData>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let ping_command = CreateCommand::new("ping").description("A ping command");
        
        let commands = vec![ping_command];

        if let Err(why) = Command::set_global_commands(&ctx.http, commands).await {
            error!("Failed to register slash commands: {}", why);
        } else {
            info!("Successfully registered slash commands");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            debug!("Received slash command: {}", command.data.name);
            
            let content = match command.data.name.as_str() {
                "ping" => {
                    info!("Ping command executed by user {}", command.user.id);
                    "Pong!".to_string()
                },
                _ => {
                    warn!("Unknown command received: {}", command.data.name);
                    "Unknown command".to_string()
                },
            };

            let data = CreateInteractionResponseMessage::new().content(content);
            let builder = CreateInteractionResponse::Message(data);

            if let Err(why) = command.create_response(&ctx.http, builder).await {
                error!("Failed to respond to slash command '{}': {}", command.data.name, why);
            } else {
                debug!("Successfully responded to slash command '{}'", command.data.name);
            }
        }
    }
}

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

    let shared_data = Arc::new(RwLock::new(bot_data));

    // Validate environment variables
    let token = std::env::var("DISCORD_TOKEN")
        .map_err(|_| anyhow::anyhow!("DISCORD_TOKEN environment variable is required"))?;

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler {
        data: shared_data.clone(),
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Discord client: {}", e))?;

    info!("Bot initialized successfully, connecting to Discord...");

    // Set up graceful shutdown
    if let Err(why) = client.start().await {
        error!("Discord client error: {}", why);
        return Err(anyhow::anyhow!("Discord client failed: {}", why));
    }

    Ok(())
}