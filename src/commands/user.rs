use serenity::{
    builder::{CreateCommand, CreateCommandOption, CreateEmbed, CreateInteractionResponse},
    model::application::{CommandInteraction, CommandOptionType},
};
use crate::{commands::App, data::UserData, store::Error, utils::{command_helpers, responses}};
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

pub fn stats_command() -> CreateCommand {
    CreateCommand::new("stats")
        .description("View goal, streaks, and check-in status for yourself or another user")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::User,
                "user",
                "The user to view stats for (defaults to yourself)"
            )
            .required(false)
        )
}

pub fn thread_checkins_command() -> CreateCommand {
    CreateCommand::new("thread-checkins")
        .description("Choose whether your replies in the daily thread count as check-ins")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Boolean,
                "enabled",
                "Whether thread replies count toward your streak (the button always counts)"
            )
            .required(true)
        )
}

pub async fn register_goal(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    // Extract context and arguments using helper functions with ? operator
    let user_id = command_helpers::get_user_id(command);
    let guild_id = command_helpers::get_guild_id(command)?;
    let goal = command_helpers::get_string_option(command, "goal")?;

    info!("Register goal command executed by user {}", user_id);

    // Validate goal length
    if goal.chars().count() > 500 {
        return Ok(responses::default_response("Goal must be 500 characters or less."));
    }

    let now = Utc::now();
    let is_update;

    // Update or create user data
    let user_data = match app.store.get_user(guild_id, user_id).await? {
        Some(mut existing_user) => {
            if existing_user.is_active {
                // Update existing active user - preserve all streak data
                existing_user.goal = goal.clone();
                existing_user.updated_at = now;
                is_update = true;
            } else {
                // Reactivate inactive user - reset streak, optionally update goal
                existing_user.goal = goal.clone();
                existing_user.current_streak = 0;
                existing_user.last_checkin_date = None;
                existing_user.grace_period_start = None;
                existing_user.is_active = true;
                existing_user.updated_at = now;
                is_update = false; // Treat as new registration for messaging
            }
            existing_user
        }
        None => {
            // Create new user
            is_update = false;
            UserData {
                guild_id,
                user_id,
                goal: goal.clone(),
                current_streak: 0,
                longest_streak: 0,
                last_checkin_date: None,
                grace_period_start: None,
                is_active: true,
                thread_checkins: true,
                created_at: now,
                updated_at: now,
            }
        }
    };

    if let Err(e) = app.store.put_user(&user_data).await {
        error!("Failed to save user data: {}", e);
        return Ok(responses::default_response("Failed to save your goal. Please try again."));
    }

    // Send success response
    let message = if is_update {
        format!("Your goal has been updated to: \"{}\"", goal)
    } else {
        format!("Your goal has been set to: **{}**\n\nYou'll be pinged for daily check-ins to track your progress!", goal)
    };

    info!("Successfully {} goal for user {} in guild {}",
          if is_update { "updated" } else { "registered" },
          user_id,
          guild_id);

    Ok(responses::default_response(&message))
}

pub async fn edit_goal(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    // /edit-goal is an alias for /register-goal - same functionality, clearer intent
    register_goal(app, command).await
}

pub async fn deregister(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    // Extract context using helper functions
    let user_id = command_helpers::get_user_id(command);
    let guild_id = command_helpers::get_guild_id(command)?;

    info!("Deregister command executed by user {}", user_id);

    // Deactivate user (preserve data for potential re-registration)
    let mut existing_user = match app.store.get_user(guild_id, user_id).await? {
        Some(user) if user.is_active => user,
        _ => return Err(serenity::Error::Other("You're not currently registered for daily check-ins").into()),
    };

    let current_streak = existing_user.current_streak;
    existing_user.is_active = false;
    existing_user.updated_at = Utc::now();

    if let Err(e) = app.store.put_user(&existing_user).await {
        error!("Failed to save user data: {}", e);
        return Ok(responses::default_response("Failed to remove your registration. Please try again."));
    }

    info!("Successfully deactivated user {} in guild {}", user_id, guild_id);

    let message = format!("You have been removed from daily check-ins. Your streak was {} days. Use `/register-goal` to re-register later if you'd like.", current_streak);
    Ok(responses::default_response(&message))
}

pub async fn stats(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    use chrono::Duration;
    use serenity::model::application::CommandDataOptionValue;

    let guild_id = command_helpers::get_guild_id(command)?;

    // Check if a user parameter was provided, otherwise use the command user
    let (target_user_id, is_self) = command.data.options.iter()
        .find(|opt| opt.name == "user")
        .and_then(|opt| match &opt.value {
            CommandDataOptionValue::User(user_id) => Some((user_id.get(), *user_id == command.user.id)),
            _ => None,
        })
        .unwrap_or_else(|| (command_helpers::get_user_id(command), true));

    info!("Stats command executed by user {} for user {}", command_helpers::get_user_id(command), target_user_id);

    // Get user data
    let user = match app.store.get_user(guild_id, target_user_id).await? {
        Some(user) if user.is_active => user,
        _ => {
            let msg = if is_self {
                "You're not currently registered for daily check-ins. Use `/register-goal` to get started!"
            } else {
                "That user is not currently registered for daily check-ins."
            };
            return Ok(responses::default_response(msg));
        }
    };

    // Build the stats embed
    let title = if is_self {
        "📊 Your Stats"
    } else {
        "📊 User Stats"
    };

    let mut embed = CreateEmbed::new()
        .title(title)
        .color(0x00d4ff); // Light blue color

    // Add user mention if not self
    if !is_self {
        embed = embed.description(format!("<@{}>", target_user_id));
    }

    // Goal field
    embed = embed.field("🎯 Goal", &user.goal, false);

    // Streak fields
    embed = embed
        .field("🔥 Current Streak", format!("{} days", user.current_streak), true)
        .field("🏆 Longest Streak", format!("{} days", user.longest_streak), true);

    // Check-in status field
    let checkin_status = if let Some(daily_post) = app.store.get_post(guild_id).await? {
        let post_date = daily_post.posted_at.date_naive();
        let now = Utc::now();

        // Check if user has checked in today
        let has_checked_in_today = user.last_checkin_date
            .map(|last_checkin| last_checkin >= post_date)
            .unwrap_or(false);

        if has_checked_in_today {
            "✅ Complete".to_string()
        } else {
            // Calculate time remaining
            let deadline = daily_post.posted_at + Duration::hours(24);
            let time_remaining = deadline.signed_duration_since(now);

            if time_remaining.num_seconds() > 0 {
                let deadline_unix = deadline.timestamp();
                format!("⏳ Not yet complete\n**Streak expires:** <t:{}:R>", deadline_unix)
            } else {
                "❌ Missed (deadline passed)".to_string()
            }
        }
    } else {
        "No daily post yet for today".to_string()
    };

    embed = embed.field("📅 Today's Check-in", checkin_status, false);

    info!("Successfully displayed stats for user {} in guild {}", target_user_id, guild_id);
    Ok(responses::embed_response(embed))
}

pub async fn thread_checkins(
    app: &App,
    command: &CommandInteraction,
) -> Result<CreateInteractionResponse, Error> {
    let user_id = command_helpers::get_user_id(command);
    let guild_id = command_helpers::get_guild_id(command)?;
    let enabled = command_helpers::get_bool_option(command, "enabled")?;

    info!("Thread checkins command executed by user {} (enabled: {})", user_id, enabled);

    let mut user = match app.store.get_user(guild_id, user_id).await? {
        Some(user) if user.is_active => user,
        _ => return Err(serenity::Error::Other("You're not currently registered for daily check-ins. Use `/register-goal` to get started!").into()),
    };

    user.thread_checkins = enabled;
    user.updated_at = Utc::now();

    if let Err(e) = app.store.put_user(&user).await {
        error!("Failed to save user data: {}", e);
        return Ok(responses::ephemeral_response("Failed to save your setting. Please try again."));
    }

    let message = if enabled {
        "Your replies in the daily thread will now count as check-ins. The 🔥 **Check in** button still works too."
    } else {
        "Your replies in the daily thread will no longer count as check-ins. Use the 🔥 **Check in** button to keep your streak."
    };
    Ok(responses::ephemeral_response(message))
}
