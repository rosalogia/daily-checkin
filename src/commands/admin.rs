use serenity::{
    builder::{CreateCommand, CreateCommandOption},
    model::{
        application::{CommandInteraction, CommandOptionType},
        id::{ChannelId, GuildId, UserId},
    },
    prelude::*,
};
use crate::{
    bot::SharedBotData,
    data::ServerConfig,
    utils::{
        command_helpers::{get_guild_id, get_channel_option, get_string_option, is_admin, validate_timezone, validate_time_format},
        responses::{default_response},
    },
    streaks::StreakManager,
};
use chrono::Utc;
use tracing::{info, debug, error};

pub fn set_channel_command() -> CreateCommand {
    CreateCommand::new("set-checkin-channel")
        .description("Configure the daily check-in channel (Admin only)")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "channel",
                "The channel for daily check-in messages"
            )
            .required(true)
        )
}

pub async fn set_channel(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    info!("Set checkin channel command executed by user {}", command.user.id);
    
    // Check admin permissions
    if !is_admin(ctx, command).await? {
        let response = default_response("This command requires administrator permissions.");
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }
    
    // Get guild ID and channel ID
    let guild_id = get_guild_id(command)?;
    let channel_id = get_channel_option(command, "channel")?;
    
    // Update server configuration
    {
        let mut bot_data = data.write().await;
        
        // Get existing server config or create new one
        let mut server_config = bot_data
            .get_server_config(&guild_id)
            .cloned()
            .unwrap_or_else(|| ServerConfig {
                guild_id: guild_id.clone(),
                checkin_channel_id: None,
                timezone: "UTC".to_string(), // Default timezone
                daily_time: "09:00".to_string(), // Default time
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        
        // Update the channel ID and timestamp
        server_config.checkin_channel_id = Some(channel_id.to_string());
        server_config.updated_at = Utc::now();
        
        // Save to data store
        bot_data.add_or_update_server(server_config);
        
        // Persist to disk
        if let Err(e) = bot_data.save().await {
            error!("Failed to save data after setting checkin channel: {}", e);
            let response = default_response("Failed to save configuration. Please try again.");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }
    
    debug!("Successfully configured checkin channel {} for guild {}", channel_id, guild_id);
    
    let response = default_response(&format!("Daily check-in channel has been set to <#{}>!", channel_id));
    command.create_response(&ctx.http, response).await?;
    Ok(())
}

pub fn set_checkin_time_command() -> CreateCommand {
    CreateCommand::new("set-checkin-time")
        .description("Configure the daily check-in time and timezone (Admin only)")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "time",
                "Time in HH:MM format (e.g., 09:00, 13:30)"
            )
            .required(true)
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "timezone",
                "Timezone (e.g., America/New_York, Europe/London, UTC)"
            )
            .required(false)
        )
}

pub async fn set_checkin_time(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    info!("Set checkin time command executed by user {}", command.user.id);
    
    // Check admin permissions
    if !is_admin(ctx, command).await? {
        let response = default_response("This command requires administrator permissions.");
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }
    
    // Get guild ID
    let guild_id = get_guild_id(command)?;
    
    // Get and validate time
    let time_str = get_string_option(command, "time")?;
    let validated_time = match validate_time_format(&time_str) {
        Ok(time) => time,
        Err(e) => {
            error!("Invalid time format: {}", e);
            let response = default_response("Invalid time format. Please use HH:MM format (e.g., '09:00', '13:30').");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    };
    
    // Get and validate timezone (optional)
    let validated_timezone = if let Ok(timezone_str) = get_string_option(command, "timezone") {
        match validate_timezone(&timezone_str) {
            Ok(tz) => tz,
            Err(e) => {
                error!("Invalid timezone: {}", e);
                let response = default_response("Invalid timezone. Use format like 'America/New_York', 'Europe/London', or 'UTC'.");
                command.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        }
    } else {
        // Keep existing timezone or default to UTC
        "UTC".to_string()
    };
    
    // Update server configuration
    {
        let mut bot_data = data.write().await;
        
        // Get existing server config or create new one
        let mut server_config = bot_data
            .get_server_config(&guild_id)
            .cloned()
            .unwrap_or_else(|| ServerConfig {
                guild_id: guild_id.clone(),
                checkin_channel_id: None,
                timezone: "UTC".to_string(),
                daily_time: "09:00".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        
        // Update the time and timezone
        server_config.daily_time = validated_time.clone();
        if command.data.options.iter().any(|opt| opt.name == "timezone") {
            server_config.timezone = validated_timezone.clone();
        }
        server_config.updated_at = Utc::now();
        
        // Save to data store
        bot_data.add_or_update_server(server_config);
        
        // Persist to disk
        if let Err(e) = bot_data.save().await {
            error!("Failed to save data after setting checkin time: {}", e);
            let response = default_response("Failed to save configuration. Please try again.");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }
    
    debug!("Successfully configured checkin time {} {} for guild {}", validated_time, validated_timezone, guild_id);
    
    let response = if command.data.options.iter().any(|opt| opt.name == "timezone") {
        default_response(&format!("Daily check-in time has been set to {} {} timezone!", validated_time, validated_timezone))
    } else {
        default_response(&format!("Daily check-in time has been set to {}!", validated_time))
    };
    command.create_response(&ctx.http, response).await?;
    Ok(())
}

pub fn trigger_checkin_command() -> CreateCommand {
    CreateCommand::new("trigger-checkin")
        .description("Manually trigger the daily check-in post (Bot owner only)")
}

pub async fn trigger_checkin(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    const AUTHORIZED_USER_ID: u64 = 586202950116966402;

    info!("Trigger checkin command executed by user {}", command.user.id);

    // Check if user is authorized
    if command.user.id != UserId::new(AUTHORIZED_USER_ID) {
        let response = default_response("This command is restricted to the bot owner.");
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Get guild ID
    let guild_id = get_guild_id(command)?;

    // Get server configuration
    let channel_id = {
        let bot_data = data.read().await;
        match bot_data.get_server_config(&guild_id) {
            Some(config) => {
                match &config.checkin_channel_id {
                    Some(id) => id.clone(),
                    None => {
                        let response = default_response("No check-in channel configured. Use `/set-checkin-channel` first.");
                        command.create_response(&ctx.http, response).await?;
                        return Ok(());
                    }
                }
            }
            None => {
                let response = default_response("Server not configured. Use `/set-checkin-channel` first.");
                command.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        }
    };

    // Acknowledge the command
    let response = default_response("Triggering daily check-in post...");
    command.create_response(&ctx.http, response).await?;

    let guild_id_parsed: GuildId = guild_id.parse()
        .map_err(|_| serenity::Error::Other("Invalid guild ID"))?;

    // Credit thread replies to the previous post before streak maintenance
    match StreakManager::new(data.clone()).process_thread_replies(ctx, guild_id_parsed).await {
        Ok(count) => info!("Credited {} thread check-ins for guild {} before manual post", count, guild_id),
        Err(e) => error!("Failed to process thread replies for guild {}: {}", guild_id, e),
    }

    // Run streak maintenance
    {
        let mut bot_data = data.write().await;
        match StreakManager::reset_streaks_for_guild(&mut bot_data, &guild_id).await {
            Ok(reset_count) => {
                if reset_count > 0 {
                    info!("Reset {} streaks for guild {} before manual post", reset_count, guild_id);
                }
            }
            Err(e) => {
                error!("Failed to run streak maintenance for guild {}: {}", guild_id, e);
            }
        }

        if let Err(e) = bot_data.save().await {
            error!("Failed to save data after streak maintenance: {}", e);
        }
    }

    // Parse channel ID and post the message
    let channel_id_parsed: ChannelId = channel_id.parse()
        .map_err(|_| serenity::Error::Other("Invalid channel ID"))?;

    // Use the scheduler's post method
    let scheduler = crate::scheduler::DailyScheduler::new(data.clone());
    match scheduler.post_daily_message(ctx, guild_id_parsed, channel_id_parsed).await {
        Ok(_) => {
            info!("Successfully posted manual daily message for guild {}", guild_id);
            // Follow up with success message
            command.edit_response(&ctx.http,
                serenity::builder::EditInteractionResponse::new()
                    .content("✅ Daily check-in post created successfully!")
            ).await?;
        }
        Err(e) => {
            error!("Failed to post manual daily message: {}", e);
            command.edit_response(&ctx.http,
                serenity::builder::EditInteractionResponse::new()
                    .content("❌ Failed to create daily check-in post. Check bot logs for details.")
            ).await?;
        }
    }

    Ok(())
}
