pub mod ping;
pub mod user;
pub mod admin;

use aws_sdk_lambda::{primitives::Blob, types::InvocationType};
use serenity::{
    builder::{CreateCommand, CreateInteractionResponse},
    model::application::{ComponentInteraction, Interaction},
};
use crate::{
    daily::DailyEvent,
    store::{Error, Store},
    streaks::{StreakManager, CHECKIN_BUTTON_ID},
    utils::responses::ephemeral_response,
};
use tracing::{error, warn};

/// Shared state for handling interactions
pub struct App {
    pub store: Store,
    pub lambda: aws_sdk_lambda::Client,
    /// Name of the daily Lambda function, invoked for manual check-in posts
    pub daily_function: String,
}

impl App {
    /// Invoke the daily function asynchronously
    pub async fn trigger_daily(&self, event: &DailyEvent) -> Result<(), Error> {
        self.lambda
            .invoke()
            .function_name(&self.daily_function)
            .invocation_type(InvocationType::Event)
            .payload(Blob::new(serde_json::to_vec(event)?))
            .send()
            .await?;
        Ok(())
    }
}

pub fn all_commands() -> Vec<CreateCommand> {
    vec![
        ping::register(),
        user::register_goal_command(),
        user::edit_goal_command(),
        user::deregister_command(),
        user::stats_command(),
        admin::set_channel_command(),
        admin::set_checkin_time_command(),
        admin::trigger_checkin_command(),
    ]
}

/// Handle an interaction and build the response to send back to Discord
pub async fn handle_interaction(app: &App, interaction: &Interaction) -> CreateInteractionResponse {
    let result = match interaction {
        Interaction::Ping(_) => Ok(CreateInteractionResponse::Pong),
        Interaction::Command(command) => match command.data.name.as_str() {
            "ping" => Ok(ping::run(command)),
            "register-goal" => user::register_goal(app, command).await,
            "edit-goal" => user::edit_goal(app, command).await,
            "deregister" => user::deregister(app, command).await,
            "stats" => user::stats(app, command).await,
            "set-checkin-channel" => admin::set_channel(app, command).await,
            "set-checkin-time" => admin::set_checkin_time(app, command).await,
            "trigger-checkin" => admin::trigger_checkin(app, command).await,
            _ => {
                warn!("Unknown command: {}", command.data.name);
                Ok(ephemeral_response("Unknown command."))
            }
        },
        Interaction::Component(component) => match component.data.custom_id.as_str() {
            CHECKIN_BUTTON_ID => handle_checkin_button(app, component).await,
            _ => {
                warn!("Unknown component: {}", component.data.custom_id);
                Ok(ephemeral_response("Unknown action."))
            }
        },
        _ => {
            warn!("Unhandled interaction type: {:?}", interaction.kind());
            Ok(ephemeral_response("Unsupported interaction."))
        }
    };

    result.unwrap_or_else(|why| {
        // User-facing validation errors are raised as serenity::Error::Other
        if let Some(serenity::Error::Other(message)) = why.downcast_ref::<serenity::Error>() {
            return ephemeral_response(message);
        }
        error!("Error handling interaction: {}", why);
        ephemeral_response("❌ Something went wrong. Please try again.")
    })
}

async fn handle_checkin_button(app: &App, component: &ComponentInteraction) -> Result<CreateInteractionResponse, Error> {
    let reply = StreakManager::new(app.store.clone()).process_button(component).await?;

    // Reply privately so check-ins don't clutter the channel
    Ok(ephemeral_response(&reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_endpoint_verification_ping() {
        // Discord sends this when the Interactions Endpoint URL is saved
        let payload = r#"{"application_id":"123","id":"456","token":"abc","type":1,"version":1,"user":{"id":"789","username":"x","discriminator":"0","avatar":null}}"#;
        let interaction: Interaction = serde_json::from_str(payload).unwrap();
        assert!(matches!(interaction, Interaction::Ping(_)));

        let pong = serde_json::to_value(CreateInteractionResponse::Pong).unwrap();
        assert_eq!(pong["type"], 1);
    }
}
