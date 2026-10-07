use serenity::{
    builder::{CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage},
    model::{
        application::{CommandInteraction, CommandOptionType},
        id::UserId,
    },
};
use crate::{
    commands::App,
    daily::DailyEvent,
    data::ServerConfig,
    store::Error,
    utils::{
        command_helpers::{get_guild_id, get_channel_option, get_string_option, is_admin, validate_timezone, validate_time_format},
        responses::{default_response},
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
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    info!("Set checkin channel command executed by user {}", command.user.id);
    
    // Check admin permissions
    if !is_admin(command) {
        return Ok(default_response("This command requires administrator permissions."));
    }
    
    // Get guild ID and channel ID
    let guild_id = get_guild_id(command)?;
    let channel_id = get_channel_option(command, "channel")?;
    
    // Get existing server config or create new one
    let mut server_config = app.store.get_config(guild_id).await?
        .unwrap_or_else(|| ServerConfig::new(guild_id));

    // Update the channel ID and timestamp
    server_config.checkin_channel_id = Some(channel_id.get());
    server_config.updated_at = Utc::now();

    if let Err(e) = app.store.save_config(&server_config).await {
        error!("Failed to save data after setting checkin channel: {}", e);
        return Ok(default_response("Failed to save configuration. Please try again."));
    }

    debug!("Successfully configured checkin channel {} for guild {}", channel_id, guild_id);
    
    Ok(default_response(&format!("Daily check-in channel has been set to <#{}>!", channel_id)))
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
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    info!("Set checkin time command executed by user {}", command.user.id);
    
    // Check admin permissions
    if !is_admin(command) {
        return Ok(default_response("This command requires administrator permissions."));
    }
    
    // Get guild ID
    let guild_id = get_guild_id(command)?;
    
    // Get and validate time
    let time_str = get_string_option(command, "time")?;
    let validated_time = match validate_time_format(&time_str) {
        Ok(time) => time,
        Err(e) => {
            error!("Invalid time format: {}", e);
            return Ok(default_response("Invalid time format. Please use HH:MM format (e.g., '09:00', '13:30')."));
        }
    };
    
    // Get and validate timezone (optional)
    let validated_timezone = if let Ok(timezone_str) = get_string_option(command, "timezone") {
        match validate_timezone(&timezone_str) {
            Ok(tz) => tz,
            Err(e) => {
                error!("Invalid timezone: {}", e);
                return Ok(default_response("Invalid timezone. Use format like 'America/New_York', 'Europe/London', or 'UTC'."));
            }
        }
    } else {
        // Keep existing timezone or default to UTC
        "UTC".to_string()
    };
    
    // Get existing server config or create new one
    let mut server_config = app.store.get_config(guild_id).await?
        .unwrap_or_else(|| ServerConfig::new(guild_id));

    // Update the time and timezone
    server_config.daily_time = validated_time.clone();
    if command.data.options.iter().any(|opt| opt.name == "timezone") {
        server_config.timezone = validated_timezone.clone();
    }
    server_config.updated_at = Utc::now();

    if let Err(e) = app.store.save_config(&server_config).await {
        error!("Failed to save data after setting checkin time: {}", e);
        return Ok(default_response("Failed to save configuration. Please try again."));
    }

    debug!("Successfully configured checkin time {} {} for guild {}", validated_time, validated_timezone, guild_id);
    
    let response = if command.data.options.iter().any(|opt| opt.name == "timezone") {
        default_response(&format!("Daily check-in time has been set to {} {} timezone!", validated_time, validated_timezone))
    } else {
        default_response(&format!("Daily check-in time has been set to {}!", validated_time))
    };
    Ok(response)
}

pub fn trigger_checkin_command() -> CreateCommand {
    CreateCommand::new("trigger-checkin")
        .description("Manually trigger the daily check-in post (Bot owner only)")
}

pub async fn trigger_checkin(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    const AUTHORIZED_USER_ID: u64 = 586202950116966402;

    info!("Trigger checkin command executed by user {}", command.user.id);

    // Check if user is authorized
    if command.user.id != UserId::new(AUTHORIZED_USER_ID) {
        return Ok(default_response("This command is restricted to the bot owner."));
    }

    // Get guild ID
    let guild_id = get_guild_id(command)?;

    // Get server configuration
    let channel_id = match app.store.get_config(guild_id).await? {
        Some(config) => {
            match config.checkin_channel_id {
                Some(id) => id,
                None => return Ok(default_response("No check-in channel configured. Use `/set-checkin-channel` first.")),
            }
        }
        None => return Ok(default_response("Server not configured. Use `/set-checkin-channel` first.")),
    };

    // Posting takes longer than Discord's 3-second response deadline, so hand it to the daily
    // function, which edits this deferred response when it finishes
    let event = DailyEvent::Manual {
        guild_id,
        channel_id,
        application_id: command.application_id.get(),
        interaction_token: command.token.clone(),
    };
    if let Err(e) = app.trigger_daily(&event).await {
        error!("Failed to trigger daily function: {}", e);
        return Ok(default_response("❌ Failed to trigger the daily check-in post. Check logs for details."));
    }

    Ok(CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()))
}
