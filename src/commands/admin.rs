use serenity::{
    builder::{CreateCommand, CreateCommandOption},
    model::application::{CommandInteraction, CommandOptionType},
    prelude::*,
};
use crate::{
    bot::SharedBotData,
    data::ServerConfig,
    utils::{
        command_helpers::{get_guild_id, get_channel_option, get_string_option, is_admin, validate_timezone, validate_time_format},
        responses::{success_response, error_response},
    },
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
        let response = error_response("This command requires administrator permissions.");
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
            let response = error_response("Failed to save configuration. Please try again.");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }
    
    debug!("Successfully configured checkin channel {} for guild {}", channel_id, guild_id);
    
    let response = success_response(&format!("Daily check-in channel has been set to <#{}>!", channel_id));
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
        let response = error_response("This command requires administrator permissions.");
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
            let response = error_response("Invalid time format. Please use HH:MM format (e.g., '09:00', '13:30').");
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
                let response = error_response("Invalid timezone. Use format like 'America/New_York', 'Europe/London', or 'UTC'.");
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
            let response = error_response("Failed to save configuration. Please try again.");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }
    
    debug!("Successfully configured checkin time {} {} for guild {}", validated_time, validated_timezone, guild_id);
    
    let response = if command.data.options.iter().any(|opt| opt.name == "timezone") {
        success_response(&format!("Daily check-in time has been set to {} {} timezone!", validated_time, validated_timezone))
    } else {
        success_response(&format!("Daily check-in time has been set to {}!", validated_time))
    };
    command.create_response(&ctx.http, response).await?;
    Ok(())
}