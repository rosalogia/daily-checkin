use crate::{bot::SharedBotData, data::DailyPost, streaks::StreakManager};
use chrono::{DateTime, Utc, NaiveTime, Timelike};
use chrono_tz::Tz;
use serenity::{
    builder::{CreateMessage, CreateThread, CreateEmbed},
    model::id::{ChannelId, GuildId},
    prelude::*,
};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, debug};

pub struct DailyScheduler {
    data: SharedBotData,
}

impl DailyScheduler {
    pub fn new(data: SharedBotData) -> Self {
        Self { data }
    }

    /// Start the daily scheduler loop
    pub async fn start(&self, ctx: Context) {
        info!("Starting daily scheduler");
        
        loop {
            // Check every 60 seconds if it's time to post
            sleep(Duration::from_secs(60)).await;
            
            if let Err(e) = self.check_and_post_daily_messages(&ctx).await {
                error!("Error in daily scheduler: {}", e);
            }
        }
    }

    /// Check all servers and post daily messages if it's time
    async fn check_and_post_daily_messages(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = self.data.write().await;
        let now = Utc::now();
        
        // Clone the servers map to avoid borrow checker issues
        let servers = data.servers.clone();
        
        for (guild_id, server_config) in &servers {
            // Skip if no channel configured
            let channel_id = match &server_config.checkin_channel_id {
                Some(id) => id,
                None => {
                    debug!("No checkin channel configured for guild {}", guild_id);
                    continue;
                }
            };

            // Check if it's time to post for this server
            if self.is_time_to_post(&server_config.daily_time, &server_config.timezone, now).await? {
                // Check if we already posted recently
                if self.already_posted_recently(&data, guild_id, now) {
                    debug!("Already posted recently for guild {}", guild_id);
                    continue;
                }

                info!("Posting daily message for guild {} in channel {}", guild_id, channel_id);
                
                // Run streak maintenance inline
                match StreakManager::reset_streaks_for_guild(&mut data, guild_id).await {
                    Ok(reset_count) => {
                        if reset_count > 0 {
                            info!("Reset {} streaks for guild {} before daily post", reset_count, guild_id);
                        }
                    }
                    Err(e) => {
                        error!("Failed to run streak maintenance for guild {}: {}", guild_id, e);
                    }
                }
                
                // Save data after streak maintenance
                if let Err(e) = data.save().await {
                    error!("Failed to save data after streak maintenance for guild {}: {}", guild_id, e);
                }
                
                let guild_id_parsed: GuildId = guild_id.parse()?;
                let channel_id_parsed: ChannelId = channel_id.parse()?;
                
                // Release the write lock before posting
                drop(data);
                
                self.post_daily_message(ctx, guild_id_parsed, channel_id_parsed).await?;
                
                // Re-acquire the write lock for the next iteration
                data = self.data.write().await;
            }
        }

        Ok(())
    }

    /// Check if it's time to post based on server timezone and configured time
    async fn is_time_to_post(
        &self,
        daily_time: &str,
        timezone: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Parse the configured time
        let target_time = NaiveTime::parse_from_str(daily_time, "%H:%M")?;
        
        // Parse the timezone
        let tz: Tz = timezone.parse()?;
        
        // Convert current UTC time to server timezone
        let local_now = now.with_timezone(&tz);
        let local_time = local_now.time();
        
        // Check if current time matches target time (within 1 minute)
        let target_minutes = target_time.hour() * 60 + target_time.minute();
        let current_minutes = local_time.hour() * 60 + local_time.minute();
        
        Ok((current_minutes as i32 - target_minutes as i32).abs() < 1)
    }

    /// Check if we already posted recently for a guild (within last 20 hours to prevent double posting)
    fn already_posted_recently(
        &self,
        data: &crate::data::BotData,
        guild_id: &str,
        now: DateTime<Utc>,
    ) -> bool {
        if let Some(post) = data.daily_posts.get(guild_id) {
            let hours_since_post = now.signed_duration_since(post.posted_at).num_hours();
            hours_since_post < 20 // Prevent posting again too soon
        } else {
            false
        }
    }

    /// Post the daily check-in message
    async fn post_daily_message(
        &self,
        ctx: &Context,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Generate the daily message embed
        let embed = self.generate_daily_embed(guild_id).await?;
        
        // Post the message
        let message = channel_id.send_message(&ctx.http, CreateMessage::new().add_embed(embed)).await?;
        
        // Create a thread under the message with today's date
        let today = Utc::now().format("%m/%d/%y");
        let thread_name = format!("Daily Check-in Responses {}", today);
        let thread = message
            .channel_id
            .create_thread(&ctx.http,
                           CreateThread::new(thread_name).kind(serenity::model::channel::ChannelType::PublicThread)
            ).await?;
        
        // Send a ping message in the thread to notify all participants
        self.send_thread_pings(ctx, thread.id, guild_id).await?;
        
        // Save the daily post record
        {
            let mut data = self.data.write().await;
            let now = Utc::now();
            let daily_post = DailyPost {
                guild_id: guild_id.to_string(),
                channel_id: channel_id.to_string(),
                message_id: message.id.to_string(),
                thread_id: Some(thread.id.to_string()),
                posted_at: now, // When the post was actually created
                created_at: now,
            };
            
            data.daily_posts.insert(guild_id.to_string(), daily_post);
                
            if let Err(e) = data.save().await {
                error!("Failed to save daily post data: {}", e);
            }
        }
        
        info!("Successfully posted daily message for guild {} with thread {}", guild_id, thread.id);
        Ok(())
    }

    /// Generate the daily message embed with user pings, goals, and streaks
    async fn generate_daily_embed(
        &self,
        guild_id: GuildId,
    ) -> Result<CreateEmbed, Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.read().await;
        let guild_id_str = guild_id.to_string();
        
        // Get users for this guild
        let empty_map = std::collections::HashMap::new();
        let users = data.users.get(&guild_id_str).unwrap_or(&empty_map);
        
        // Filter active users
        let active_users: Vec<_> = users.values().filter(|user| user.is_active).collect();
        
        let mut embed = CreateEmbed::new()
            .title("🌅 Daily Check-in Time!")
            .description("Time to share your progress! Reply in this thread with your update.")
            .color(0x00ff88); // Green color for daily check-ins
        
        if active_users.is_empty() {
            embed = embed.field("No Users Registered", "Use `/register-goal` to join!", false);
            return Ok(embed);
        }
        
        // Sort users by streak (highest first) for motivation
        let mut sorted_users = active_users;
        sorted_users.sort_by(|a, b| b.current_streak.cmp(&a.current_streak));
        
        // Build user list for the field
        let mut user_list = String::new();
        for user in sorted_users {
            let user_mention = format!("<@{}>", user.user_id);

            // Truncate goal if it's too long for readability
            let goal_display = if user.goal.len() > 50 {
                format!("{}...", &user.goal[..47])
            } else {
                user.goal.clone()
            };
            
            user_list.push_str(&format!("• {} - {} 🔥{}\n", user_mention, goal_display, user.current_streak));
        }
        
        embed = embed
            .field("📋 Today's Participants", user_list, false)
            .footer(serenity::builder::CreateEmbedFooter::new("💪 Keep up the momentum!"));
        
        Ok(embed)
    }

    /// Send ping message to thread to notify all participants
    async fn send_thread_pings(
        &self,
        ctx: &Context,
        thread_id: serenity::model::id::ChannelId,
        guild_id: GuildId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.read().await;
        let guild_id_str = guild_id.to_string();
        
        // Get users for this guild
        let empty_map = std::collections::HashMap::new();
        let users = data.users.get(&guild_id_str).unwrap_or(&empty_map);
        
        // Filter active users and collect their mentions
        let active_users: Vec<_> = users.values().filter(|user| user.is_active).collect();
        
        if !active_users.is_empty() {
            let mentions: Vec<String> = active_users
                .iter()
                .map(|user| format!("<@{}>", user.user_id))
                .collect();
            
            let ping_message = format!("Time to check in!\n{}", mentions.join("\n"));
            
            // Send the ping message to the thread
            thread_id.send_message(&ctx.http, CreateMessage::new().content(ping_message)).await?;
        }
        
        Ok(())
    }
}
