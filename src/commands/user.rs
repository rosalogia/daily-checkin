use serenity::{
    builder::{CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage},
    model::application::{CommandInteraction, CommandOptionType},
    prelude::*,
};
use crate::{bot::SharedBotData, data::UserData, utils::{command_helpers, responses}};
use chrono::Utc;
use tracing::{info, error};

pub fn register_goal_command() -> CreateCommand {
    CreateCommand::new("register-goal")
        .description("Register a personal goal for daily check-ins")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "goal",
                "Your personal goal or objective"
            )
            .required(true)
            .max_length(500)
        )
}

pub fn edit_goal_command() -> CreateCommand {
    CreateCommand::new("edit-goal")
        .description("Edit your existing goal")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "goal",
                "Your updated goal or objective"
            )
            .required(true)
            .max_length(500)
        )
}

pub fn deregister_command() -> CreateCommand {
    CreateCommand::new("deregister")
        .description("Remove yourself from daily check-ins")
}

pub async fn register_goal(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    // Extract context and arguments using helper functions with ? operator
    let user_id = command_helpers::get_user_id(command);
    let guild_id = command_helpers::get_guild_id(command)?;
    let goal = command_helpers::get_string_option(command, "goal")?;

    info!("Register goal command executed by user {}", user_id);

    // Validate goal length
    if goal.len() > 500 {
        let response = responses::error_response("Goal must be 500 characters or less.");
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    let now = Utc::now();
    let is_update;

    // Update or create user data
    {
        let mut data_write = data.write().await;
        
        if let Some(existing_user) = data_write.users.get_mut(&guild_id).and_then(|guild_users| guild_users.get_mut(&user_id)) {
            // Update existing user - preserve all streak data
            existing_user.goal = goal.clone();
            existing_user.updated_at = now;
            is_update = true;
        } else {
            // Create new user
            let user_data = UserData {
                user_id: user_id.clone(),
                goal: goal.clone(),
                current_streak: 0,
                longest_streak: 0,
                last_checkin_date: None,
                grace_period_start: None,
                is_active: true,
                created_at: now,
                updated_at: now,
            };
            data_write.add_or_update_user(guild_id.clone(), user_data);
            is_update = false;
        }
        
        if let Err(e) = data_write.save().await {
            error!("Failed to save user data: {}", e);
            let response = responses::error_response("Failed to save your goal. Please try again.");
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }
    }

    // Send success response
    let message = if is_update {
        format!("Your goal has been updated to: \"{}\"", goal)
    } else {
        format!("🎯 Welcome! Your goal has been set to: \"{}\"\n\nYou'll be pinged for daily check-ins to track your progress!", goal)
    };

    let response = responses::success_response(&message);
    command.create_response(&ctx.http, response).await?;

    info!("Successfully {} goal for user {} in guild {}", 
          if is_update { "updated" } else { "registered" }, 
          user_id, 
          guild_id);

    Ok(())
}

pub async fn edit_goal(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    info!("Edit goal command executed by user {}", command.user.id);
    
    // TODO: Implement goal editing logic
    let content = "🚧 Goal editing coming soon!";
    let response = CreateInteractionResponseMessage::new().content(content);
    let builder = CreateInteractionResponse::Message(response);
    
    command.create_response(&ctx.http, builder).await?;
    Ok(())
}

pub async fn deregister(
    ctx: &Context,
    command: &CommandInteraction,
    data: SharedBotData,
) -> serenity::Result<()> {
    info!("Deregister command executed by user {}", command.user.id);
    
    // TODO: Implement deregistration logic
    let content = "🚧 Deregistration coming soon!";
    let response = CreateInteractionResponseMessage::new().content(content);
    let builder = CreateInteractionResponse::Message(response);
    
    command.create_response(&ctx.http, builder).await?;
    Ok(())
}