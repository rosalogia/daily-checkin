use crate::{bot::SharedBotData, data::DailyPost};
use chrono::{DateTime, Utc, NaiveTime, Duration as ChronoDuration, Timelike};
use chrono_tz::Tz;
use serenity::{
    builder::{CreateMessage, CreateThread},
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
            
            // Clean up old daily posts every hour (when minutes == 0)
            let now = Utc::now();
            if now.minute() == 0 {
                if let Err(e) = self.cleanup_old_daily_posts().await {
                    error!("Error cleaning up old daily posts: {}", e);
                }
            }
        }
    }

    /// Check all servers and post daily messages if it's time
    async fn check_and_post_daily_messages(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.read().await;
        let now = Utc::now();
        
        for (guild_id, server_config) in &data.servers {
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
                let guild_id_parsed: GuildId = guild_id.parse()?;
                let channel_id_parsed: ChannelId = channel_id.parse()?;
                
                // Check if we already posted today
                if self.already_posted_today(&data, guild_id, now.date_naive()).await {
                    debug!("Already posted today for guild {}", guild_id);
                    continue;
                }

                info!("Posting daily message for guild {} in channel {}", guild_id, channel_id);
                drop(data); // Release the read lock before posting
                self.post_daily_message(ctx, guild_id_parsed, channel_id_parsed).await?;
                break; // Re-acquire lock on next iteration
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

    /// Check if we already posted today for a guild
    async fn already_posted_today(
        &self,
        data: &crate::data::BotData,
        guild_id: &str,
        today: chrono::NaiveDate,
    ) -> bool {
        if let Some(posts) = data.daily_posts.get(guild_id) {
            posts.iter().any(|post| post.post_date == today)
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
        // Generate the daily message content
        let message_content = self.generate_daily_message(guild_id).await?;
        
        // Post the message
        let message = channel_id.send_message(&ctx.http, CreateMessage::new().content(message_content)).await?;
        
        // Create a thread under the message
        let thread = message
            .channel_id
            .create_thread(&ctx.http,
                           CreateThread::new("Daily Check-in Responses").kind(serenity::model::channel::ChannelType::PublicThread)
            ).await?;
        
        // Save the daily post record
        {
            let mut data = self.data.write().await;
            let daily_post = DailyPost {
                guild_id: guild_id.to_string(),
                channel_id: channel_id.to_string(),
                message_id: message.id.to_string(),
                thread_id: Some(thread.id.to_string()),
                post_date: Utc::now().date_naive(),
                created_at: Utc::now(),
            };
            
            data.daily_posts
                .entry(guild_id.to_string())
                .or_insert_with(Vec::new)
                .push(daily_post);
                
            if let Err(e) = data.save().await {
                error!("Failed to save daily post data: {}", e);
            }
        }
        
        info!("Successfully posted daily message for guild {} with thread {}", guild_id, thread.id);
        Ok(())
    }

    /// Generate the daily message content with user pings, goals, and streaks
    async fn generate_daily_message(
        &self,
        guild_id: GuildId,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.read().await;
        let guild_id_str = guild_id.to_string();
        
        // Get users for this guild
        let empty_map = std::collections::HashMap::new();
        let users = data.users.get(&guild_id_str).unwrap_or(&empty_map);
        
        // Filter active users
        let active_users: Vec<_> = users.values().filter(|user| user.is_active).collect();
        
        if active_users.is_empty() {
            return Ok("🌅 **Daily Check-in Time!**\n\nNo registered users yet. Use `/register-goal` to join!".to_string());
        }
        
        // Build the message
        let mut message = String::from("🌅 **Daily Check-in Time!**\n\nTime to share your progress! Reply in this thread with your update.\n\n");
        
        // Sort users by streak (highest first) for motivation
        let mut sorted_users = active_users;
        sorted_users.sort_by(|a, b| b.current_streak.cmp(&a.current_streak));
        
        for user in sorted_users {
            let user_mention = format!("<@{}>", user.user_id);
            let streak_text = if user.current_streak == 0 {
                "Starting fresh".to_string()
            } else {
                format!("Day {}", user.current_streak)
            };
            
            // Truncate goal if it's too long for readability
            let goal_display = if user.goal.len() > 50 {
                format!("{}...", &user.goal[..47])
            } else {
                user.goal.clone()
            };
            
            message.push_str(&format!("* {} - *{}* 🔥{}\n", user_mention, goal_display, streak_text));
        }
        
        message.push_str("\n💪 Keep up the momentum!");
        
        Ok(message)
    }

    /// Clean up daily post records older than 48 hours
    async fn cleanup_old_daily_posts(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = self.data.write().await;
        let cutoff_date = (Utc::now() - ChronoDuration::days(2)).date_naive();
        let mut cleaned_count = 0;
        
        for (guild_id, posts) in data.daily_posts.iter_mut() {
            let original_len = posts.len();
            posts.retain(|post| post.post_date >= cutoff_date);
            let removed = original_len - posts.len();
            cleaned_count += removed;
            
            if removed > 0 {
                debug!("Cleaned {} old daily posts for guild {}", removed, guild_id);
            }
        }
        
        // Remove empty guild entries
        data.daily_posts.retain(|_, posts| !posts.is_empty());
        
        if cleaned_count > 0 {
            info!("Cleaned up {} old daily post records", cleaned_count);
            
            // Save the updated data
            if let Err(e) = data.save().await {
                error!("Failed to save data after cleanup: {}", e);
                return Err(e.into());
            }
        }
        
        Ok(())
    }
}
