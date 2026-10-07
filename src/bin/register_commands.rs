//! Register the bot's global slash commands with Discord. Run once after deploying, and again
//! whenever commands change:
//!
//!     DISCORD_TOKEN=... cargo run --bin register-commands

use daily_checkin_bot::{commands, load_discord_token};
use serenity::{http::Http, model::application::Command};

#[tokio::main]
async fn main() -> Result<(), daily_checkin_bot::store::Error> {
    dotenv::dotenv().ok();

    let http = Http::new(&load_discord_token().await?);
    let application = http.get_current_application_info().await?;
    http.set_application_id(application.id);

    let registered = Command::set_global_commands(&http, commands::all_commands()).await?;
    for command in registered {
        println!("Registered /{}", command.name);
    }

    Ok(())
}
