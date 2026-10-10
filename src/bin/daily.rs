//! Lambda run by EventBridge Scheduler every minute to post daily check-ins, and invoked
//! directly by /trigger-checkin for manual posts.

use daily_checkin_bot::{daily::{self, DailyEvent}, load_discord_token, store::Store};
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serenity::{builder::EditInteractionResponse, http::Http, model::id::{ApplicationId, ChannelId}};
use chrono::Utc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_runtime::tracing::init_default_subscriber();

    let store = Store::from_env().await?;
    let http = Http::new(&load_discord_token().await?);

    run(service_fn(|event: LambdaEvent<DailyEvent>| handle(&store, &http, event.payload))).await
}

async fn handle(store: &Store, http: &Http, event: DailyEvent) -> Result<(), Error> {
    match event {
        DailyEvent::Scheduled => daily::run_due_cycles(store, http).await,
        DailyEvent::Manual { guild_id, channel_id, application_id, interaction_token } => {
            info!("Running manual daily cycle for guild {}", guild_id);
            let result = match store.get_config(guild_id).await? {
                Some(config) => {
                    let cycle_date = daily::local_date(&config, Utc::now())?;
                    daily::run_cycle(store, http, guild_id, ChannelId::new(channel_id), cycle_date).await
                }
                None => Err(format!("No configuration for guild {}", guild_id).into()),
            };

            let content = match &result {
                Ok(()) => {
                    info!("Successfully posted manual daily message for guild {}", guild_id);
                    "✅ Daily check-in post created successfully!"
                }
                Err(e) => {
                    error!("Failed to post manual daily message: {}", e);
                    "❌ Failed to create daily check-in post. Check bot logs for details."
                }
            };

            // Edit the deferred /trigger-checkin response with the outcome
            http.set_application_id(ApplicationId::new(application_id));
            http.edit_original_interaction_response(
                &interaction_token,
                &EditInteractionResponse::new().content(content),
                Vec::new(),
            ).await?;

            result
        }
    }
}
