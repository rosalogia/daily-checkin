use serenity::{
    model::{
        application::{CommandDataOptionValue, CommandInteraction},
        id::ChannelId,
        permissions::Permissions,
    },
    prelude::*,
};
use chrono::NaiveTime;
use chrono_tz::Tz;

/// Extracts the guild ID from a Discord command interaction.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// 
/// # Returns
/// * `Ok(String)` - The guild ID as a string
/// * `Err(serenity::Error)` - If the command was not executed in a server
/// 
/// # Example
/// ```rust
/// let guild_id = get_guild_id(command)?;
/// ```
pub fn get_guild_id(command: &CommandInteraction) -> serenity::Result<String> {
    command
        .guild_id
        .ok_or_else(|| serenity::Error::Other("This command can only be used in a server"))
        .map(|id| id.to_string())
}

/// Extracts the user ID from a Discord command interaction.
/// 
/// This function never fails as command interactions always have a user.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// 
/// # Returns
/// * `String` - The user ID as a string
/// 
/// # Example
/// ```rust
/// let user_id = get_user_id(command);
/// ```
pub fn get_user_id(command: &CommandInteraction) -> String {
    command.user.id.to_string()
}

/// Extracts a string option value from a Discord command interaction.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// * `name` - The name of the option to extract
/// 
/// # Returns
/// * `Ok(String)` - The trimmed string value of the option
/// * `Err(serenity::Error)` - If the option is missing, empty, or not a string
/// 
/// # Example
/// ```rust
/// let goal = get_string_option(command, "goal")?;
/// ```
pub fn get_string_option(command: &CommandInteraction, name: &str) -> serenity::Result<String> {
    let option = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == name)
        .ok_or_else(|| serenity::Error::Other("Missing required argument"))?;
    
    match &option.value {
        CommandDataOptionValue::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(serenity::Error::Other("Argument cannot be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        _ => Err(serenity::Error::Other("Argument is not a string")),
    }
}

/// Extracts a channel option value from a Discord command interaction.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// * `name` - The name of the channel option to extract
/// 
/// # Returns
/// * `Ok(ChannelId)` - The channel ID
/// * `Err(serenity::Error)` - If the option is missing or not a channel
/// 
/// # Example
/// ```rust
/// let channel_id = get_channel_option(command, "channel")?;
/// ```
pub fn get_channel_option(command: &CommandInteraction, name: &str) -> serenity::Result<ChannelId> {
    let option = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == name)
        .ok_or_else(|| serenity::Error::Other("Missing required channel argument"))?;
    
    match &option.value {
        CommandDataOptionValue::Channel(id) => Ok(*id),
        _ => Err(serenity::Error::Other("Argument is not a channel")),
    }
}

/// Checks if a user has administrator permissions in the guild.
/// 
/// # Arguments
/// * `ctx` - The Discord context
/// * `command` - The Discord command interaction
/// 
/// # Returns
/// * `Ok(bool)` - Whether the user has admin permissions
/// * `Err(serenity::Error)` - If permission check fails
/// 
/// # Example
/// ```rust
/// if !is_admin(ctx, command).await? {
///     return Ok(error_response("This command requires administrator permissions."));
/// }
/// ```
pub async fn is_admin(ctx: &Context, command: &CommandInteraction) -> serenity::Result<bool> {
    let guild_id = command
        .guild_id
        .ok_or_else(|| serenity::Error::Other("This command can only be used in a server"))?;
    
    // Get the guild and member info from HTTP API (not cache-dependent)
    let guild = ctx.http.get_guild(guild_id).await?;
    let member = guild_id.member(&ctx.http, command.user.id).await?;
    
    // Check if user is the guild owner (owners always have admin)
    if guild.owner_id == command.user.id {
        return Ok(true);
    }
    
    // Check if any of the user's roles have administrator permission
    for role_id in &member.roles {
        if let Some(role) = guild.roles.get(role_id) {
            if role.permissions.contains(Permissions::ADMINISTRATOR) {
                return Ok(true);
            }
        }
    }
    
    Ok(false)
}

/// Validates and parses a timezone string.
/// 
/// # Arguments
/// * `timezone_str` - The timezone string to validate
/// 
/// # Returns
/// * `Ok(String)` - The validated timezone string
/// * `Err(serenity::Error)` - If the timezone is invalid
/// 
/// # Example
/// ```rust
/// let tz = validate_timezone("America/New_York")?;
/// ```
pub fn validate_timezone(timezone_str: &str) -> serenity::Result<String> {
    // Try to parse the timezone
    timezone_str.parse::<Tz>()
        .map_err(|_| serenity::Error::Other("Invalid timezone. Use format like 'America/New_York', 'Europe/London', or 'UTC'"))?;
    
    Ok(timezone_str.to_string())
}

/// Validates and parses a time string in HH:MM format.
/// 
/// # Arguments
/// * `time_str` - The time string to validate (e.g., "09:00", "13:30")
/// 
/// # Returns
/// * `Ok(String)` - The validated time string
/// * `Err(serenity::Error)` - If the time format is invalid
/// 
/// # Example
/// ```rust
/// let time = validate_time_format("09:30")?;
/// ```
pub fn validate_time_format(time_str: &str) -> serenity::Result<String> {
    // Try to parse the time in HH:MM format
    NaiveTime::parse_from_str(time_str, "%H:%M")
        .map_err(|_| serenity::Error::Other("Invalid time format. Use HH:MM format (e.g., '09:00', '13:30')"))?;
    
    Ok(time_str.to_string())
}
