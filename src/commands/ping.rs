use serenity::{
    builder::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage},
    model::application::CommandInteraction,
    prelude::*,
};
use tracing::{info, debug};

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("A simple ping command")
}

pub async fn run(ctx: &Context, command: &CommandInteraction) -> serenity::Result<()> {
    info!("Ping command executed by user {}", command.user.id);
    debug!("Ping command from guild: {:?}", command.guild_id);

    let content = "Pong! 🏓";
    let data = CreateInteractionResponseMessage::new().content(content);
    let builder = CreateInteractionResponse::Message(data);

    command.create_response(&ctx.http, builder).await?;
    debug!("Successfully responded to ping command");
    Ok(())
}