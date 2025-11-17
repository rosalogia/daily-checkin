use serenity::{
    async_trait,
    model::{
        application::Interaction,
        gateway::Ready,
        channel::Message,
    },
    prelude::*,
};
use tracing::{info, error};
use crate::{bot::SharedBotData, commands, scheduler::DailyScheduler, streaks::StreakManager};

pub struct Handler {
    pub data: SharedBotData,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        if let Err(why) = commands::register_commands(&ctx).await {
            error!("Failed to register slash commands: {}", why);
        } else {
            info!("Successfully registered slash commands");
        }

        // Start the daily scheduler
        let scheduler = DailyScheduler::new(self.data.clone());
        tokio::spawn(async move {
            scheduler.start(ctx).await;
        });
        info!("Daily scheduler started");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(why) = commands::handle_command(&ctx, &interaction, self.data.clone()).await {
            error!("Error handling command: {}", why);
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Process message for potential check-in responses
        let streak_manager = StreakManager::new(self.data.clone());
        if let Err(why) = streak_manager.process_message(&ctx, &msg).await {
            error!("Error processing message for streaks: {}", why);
        }
    }
}
