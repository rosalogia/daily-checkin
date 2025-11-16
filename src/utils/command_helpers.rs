use serenity::{
    model::application::{CommandDataOptionValue, CommandInteraction}
};

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
