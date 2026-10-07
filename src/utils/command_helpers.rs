use serenity::model::{
    application::{CommandDataOptionValue, CommandInteraction},
    id::ChannelId,
};
use chrono::NaiveTime;
use chrono_tz::Tz;

/// Extracts the guild ID from a Discord command interaction.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// 
/// # Returns
/// * `Ok(u64)` - The guild ID
/// * `Err(serenity::Error)` - If the command was not executed in a server
/// 
/// # Example
/// ```ignore
/// let guild_id = get_guild_id(command)?;
/// ```
pub fn get_guild_id(command: &CommandInteraction) -> serenity::Result<u64> {
    command
        .guild_id
        .ok_or_else(|| serenity::Error::Other("This command can only be used in a server"))
        .map(|id| id.get())
}

/// Extracts the user ID from a Discord command interaction.
/// 
/// This function never fails as command interactions always have a user.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// 
/// # Returns
/// * `u64` - The user ID
/// 
/// # Example
/// ```ignore
/// let user_id = get_user_id(command);
/// ```
pub fn get_user_id(command: &CommandInteraction) -> u64 {
    command.user.id.get()
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
/// ```ignore
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

/// Extracts a boolean option value from a Discord command interaction.
/// 
/// # Arguments
/// * `command` - The Discord command interaction
/// * `name` - The name of the option to extract
/// 
/// # Returns
/// * `Ok(bool)` - The value of the option
/// * `Err(serenity::Error)` - If the option is missing or not a boolean
/// 
/// # Example
/// ```ignore
/// let enabled = get_bool_option(command, "enabled")?;
/// ```
pub fn get_bool_option(command: &CommandInteraction, name: &str) -> serenity::Result<bool> {
    let option = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == name)
        .ok_or_else(|| serenity::Error::Other("Missing required argument"))?;

    match &option.value {
        CommandDataOptionValue::Boolean(b) => Ok(*b),
        _ => Err(serenity::Error::Other("Argument is not a boolean")),
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
/// ```ignore
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
/// Uses the member's resolved permissions that Discord includes in the interaction payload
/// (guild owners always have every permission), so no API calls are needed.
///
/// # Arguments
/// * `command` - The Discord command interaction
///
/// # Returns
/// * `bool` - Whether the user has admin permissions
///
/// # Example
/// ```ignore
/// if !is_admin(command) {
///     return Ok(default_response("This command requires administrator permissions."));
/// }
/// ```
pub fn is_admin(command: &CommandInteraction) -> bool {
    command
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .is_some_and(|permissions| permissions.administrator())
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
/// ```ignore
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
/// ```ignore
/// let time = validate_time_format("09:30")?;
/// ```
pub fn validate_time_format(time_str: &str) -> serenity::Result<String> {
    // Try to parse the time in HH:MM format
    NaiveTime::parse_from_str(time_str, "%H:%M")
        .map_err(|_| serenity::Error::Other("Invalid time format. Use HH:MM format (e.g., '09:00', '13:30')"))?;
    
    Ok(time_str.to_string())
}
