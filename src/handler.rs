use serenity::{
    async_trait,
    model::{
        application::Interaction,
        gateway::Ready,
    },
    prelude::*,
};
use tracing::{info, error};
use crate::{bot::SharedBotData, commands};

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
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(why) = commands::handle_command(&ctx, &interaction, self.data.clone()).await {
            error!("Error handling command: {}", why);
        }
    }
}