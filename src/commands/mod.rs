pub mod ping;
pub mod user;
pub mod admin;

use serenity::{
    model::{application::{Command, Interaction}},
    prelude::*,
};
use crate::bot::SharedBotData;

pub async fn register_commands(ctx: &Context) -> serenity::Result<()> {
    let commands = vec![
        ping::register(),
        user::register_goal_command(),
        user::edit_goal_command(),
        user::deregister_command(),
        admin::set_channel_command(),
    ];

    Command::set_global_commands(&ctx.http, commands).await?;
    Ok(())
}

pub async fn handle_command(
    ctx: &Context,
    interaction: &Interaction,
    data: SharedBotData,
) -> serenity::Result<()> {
    if let Interaction::Command(command) = interaction {
        match command.data.name.as_str() {
            "ping" => ping::run(ctx, command).await?,
            "register-goal" => user::register_goal(ctx, command, data).await?,
            "edit-goal" => user::edit_goal(ctx, command, data).await?,
            "deregister" => user::deregister(ctx, command, data).await?,
            "set-checkin-channel" => admin::set_channel(ctx, command, data).await?,
            _ => {
                tracing::warn!("Unknown command: {}", command.data.name);
            }
        }
    }
    Ok(())
}