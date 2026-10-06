pub mod ping;
pub mod user;
pub mod admin;

use serenity::{
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    model::{application::{Command, ComponentInteraction, Interaction}},
    prelude::*,
};
use crate::{bot::SharedBotData, streaks::{StreakManager, CHECKIN_BUTTON_ID}};
use tracing::error;

pub async fn register_commands(ctx: &Context) -> serenity::Result<()> {
    let commands = vec![
        ping::register(),
        user::register_goal_command(),
        user::edit_goal_command(),
        user::deregister_command(),
        user::stats_command(),
        admin::set_channel_command(),
        admin::set_checkin_time_command(),
        admin::trigger_checkin_command(),
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
            "stats" => user::stats(ctx, command, data).await?,
            "set-checkin-channel" => admin::set_channel(ctx, command, data).await?,
            "set-checkin-time" => admin::set_checkin_time(ctx, command, data).await?,
            "trigger-checkin" => admin::trigger_checkin(ctx, command, data).await?,
            _ => {
                tracing::warn!("Unknown command: {}", command.data.name);
            }
        }
    } else if let Interaction::Component(component) = interaction {
        match component.data.custom_id.as_str() {
            CHECKIN_BUTTON_ID => handle_checkin_button(ctx, component, data).await?,
            _ => {
                tracing::warn!("Unknown component: {}", component.data.custom_id);
            }
        }
    }
    Ok(())
}

async fn handle_checkin_button(
    ctx: &Context,
    component: &ComponentInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    let streak_manager = StreakManager::new(data);
    let reply = match streak_manager.process_button(component).await {
        Ok(reply) => reply,
        Err(why) => {
            error!("Error processing check-in button: {}", why);
            "❌ Something went wrong recording your check-in. Please try again.".to_string()
        }
    };

    // Reply privately so check-ins don't clutter the channel
    let response = CreateInteractionResponseMessage::new().content(reply).ephemeral(true);
    component.create_response(&ctx.http, CreateInteractionResponse::Message(response)).await
}