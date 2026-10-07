use serenity::{
    builder::{CreateCommand, CreateInteractionResponse},
    model::application::CommandInteraction,
};
use crate::utils::responses::default_response;
use tracing::{info, debug};

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("A simple ping command")
}

pub fn run(command: &CommandInteraction) -> CreateInteractionResponse {
    info!("Ping command executed by user {}", command.user.id);
    debug!("Ping command from guild: {:?}", command.guild_id);

    default_response("Pong! 🏓")
}
