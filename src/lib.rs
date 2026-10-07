pub mod commands;
pub mod daily;
pub mod data;
pub mod store;
pub mod streaks;
pub mod utils;

use store::Error;

/// Load the Discord bot token from the SSM parameter named by DISCORD_TOKEN_PARAMETER,
/// falling back to the DISCORD_TOKEN environment variable for local use
pub async fn load_discord_token() -> Result<String, Error> {
    if let Ok(parameter) = std::env::var("DISCORD_TOKEN_PARAMETER") {
        let config = aws_config::load_from_env().await;
        let output = aws_sdk_ssm::Client::new(&config)
            .get_parameter()
            .name(parameter)
            .with_decryption(true)
            .send()
            .await?;
        return output
            .parameter
            .and_then(|p| p.value)
            .ok_or_else(|| "Discord token parameter has no value".into());
    }

    std::env::var("DISCORD_TOKEN")
        .map_err(|_| "DISCORD_TOKEN_PARAMETER or DISCORD_TOKEN environment variable is required".into())
}
