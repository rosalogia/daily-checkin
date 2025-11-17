use crate::{bot::SharedBotData, data::{UserData, BotData}};
use chrono::{Utc, NaiveDate, Duration};
use serenity::{
    model::{
        channel::Message,
        id::{GuildId, ChannelId},
    },
    prelude::Context,
};
use tracing::{info, debug, error};

pub struct StreakManager {
    data: SharedBotData,
}

impl StreakManager {
    pub fn new(data: SharedBotData) -> Self {
        Self { data }
    }

    /// Process a message to check if it's a valid daily check-in response
    pub async fn process_message(&self, _ctx: &Context, msg: &Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Skip bot messages
        if msg.author.bot {
            return Ok(());
        }

        // Check if this message is in a daily check-in thread within 24 hours
        if let Some(guild_id) = msg.guild_id {
            let message_time = chrono::DateTime::<Utc>::from_timestamp(msg.timestamp.unix_timestamp(), 0)
                .unwrap_or_else(|| Utc::now());
            if self.is_valid_checkin_response(guild_id, msg.channel_id, &message_time).await {
                info!("Processing check-in response from user {} in guild {}", msg.author.id, guild_id);
                self.record_checkin(guild_id, msg.author.id, &message_time).await?;
            }
        }

        Ok(())
    }

    /// Check if a message is a valid check-in response (in thread + within 24 hours of post)
    async fn is_valid_checkin_response(&self, guild_id: GuildId, channel_id: ChannelId, message_time: &chrono::DateTime<Utc>) -> bool {
        let data = self.data.read().await;
        let guild_id_str = guild_id.to_string();
        let channel_id_str = channel_id.to_string();

        if let Some(daily_post) = data.daily_posts.get(&guild_id_str) {
            // Check if this is the correct thread
            if let Some(thread_id) = &daily_post.thread_id {
                if thread_id == &channel_id_str {
                    // Calculate 24-hour deadline: daily post time + 24 hours
                    let deadline = daily_post.posted_at + Duration::hours(24);
                    
                    // Check if message was posted before the deadline
                    return *message_time <= deadline;
                }
            }
        }

        false
    }

    /// Record a check-in and update user streak
    async fn record_checkin(
        &self,
        guild_id: GuildId,
        user_id: serenity::model::id::UserId,
        message_time: &chrono::DateTime<Utc>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = self.data.write().await;
        let guild_id_str = guild_id.to_string();
        let user_id_str = user_id.to_string();
        let response_date = message_time.date_naive();

        // Check if user already has a response for this daily post cycle (before borrowing mutably)
        let post_date = data.daily_posts.get(&guild_id_str).map(|post| post.posted_at.date_naive());
        
        // Get the user
        let user = match data.users
            .get_mut(&guild_id_str)
            .and_then(|guild_users| guild_users.get_mut(&user_id_str))
        {
            Some(user) if user.is_active => user,
            Some(_) => {
                debug!("User {} is inactive in guild {}, ignoring check-in", user_id, guild_id);
                return Ok(());
            }
            None => {
                debug!("User {} not registered in guild {}, ignoring check-in", user_id, guild_id);
                return Ok(());
            }
        };
        
        if let Some(post_date) = post_date {
            if let Some(last_checkin) = user.last_checkin_date {
                // If they already checked in on or after the day this post was created, skip
                if last_checkin >= post_date {
                    debug!("User {} already checked in for this daily post cycle in guild {}", user_id, guild_id);
                    return Ok(());
                }
            }
        }

        // Update user streak
        Self::update_user_streak(user, response_date);
        info!("User {} checked in! New streak: {} days", user_id, user.current_streak);

        // Save data
        if let Err(e) = data.save().await {
            error!("Failed to save data after recording check-in: {}", e);
            return Err(e.into());
        }

        Ok(())
    }

    /// Update a user's streak based on their check-in
    pub fn update_user_streak(user: &mut UserData, response_date: NaiveDate) {
        match user.last_checkin_date {
            None => {
                // First check-in ever
                user.current_streak = 1;
                user.last_checkin_date = Some(response_date);
            }
            Some(last_date) => {
                if last_date == response_date {
                    // Already checked in today (shouldn't happen with our duplicate check)
                    return;
                } else if last_date == response_date.pred_opt().unwrap_or(response_date) {
                    // Checked in yesterday - continue streak
                    user.current_streak += 1;
                    user.last_checkin_date = Some(response_date);
                } else if last_date < response_date.pred_opt().unwrap_or(response_date) {
                    // Missed at least one day - check for grace period
                    if Self::should_apply_grace_period(user, last_date, response_date) {
                        // Grace period applies - continue streak but mark grace period start
                        user.current_streak += 1;
                        user.last_checkin_date = Some(response_date);
                        if user.grace_period_start.is_none() {
                            user.grace_period_start = Some(last_date.succ_opt().unwrap_or(response_date));
                        }
                    } else {
                        // No grace period or grace period exceeded - reset streak
                        user.current_streak = 1;
                        user.last_checkin_date = Some(response_date);
                        user.grace_period_start = None;
                    }
                } else {
                    // Future date (shouldn't happen)
                    debug!("Warning: Check-in date in the future for user {}", user.user_id);
                }
            }
        }

        // Update longest streak if current is higher
        if user.current_streak > user.longest_streak {
            user.longest_streak = user.current_streak;
        }

        // Update timestamp
        user.updated_at = Utc::now();
    }

    /// Free function for guild-specific streak maintenance
    /// Can be called inline without needing StreakManager instance
    pub async fn reset_streaks_for_guild(data: &mut BotData, guild_id: &str) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let yesterday = Utc::now().date_naive().pred_opt().unwrap_or(Utc::now().date_naive());
        let mut reset_count = 0;

        if let Some(guild_users) = data.users.get_mut(guild_id) {
            for (user_id, user) in guild_users.iter_mut() {
                if !user.is_active {
                    continue;
                }

                // Check if user missed yesterday's check-in
                if let Some(last_checkin) = user.last_checkin_date {
                    if last_checkin < yesterday {
                        // User missed check-in, check if grace period applies
                        if !Self::should_apply_grace_period(user, last_checkin, yesterday.succ_opt().unwrap_or(yesterday)) {
                            // Reset streak
                            user.current_streak = 0;
                            user.grace_period_start = None;
                            user.updated_at = Utc::now();
                            reset_count += 1;
                            info!("Reset streak for user {} in guild {} due to missed check-in", user_id, guild_id);
                        }
                    }
                }
            }
        }

        Ok(reset_count)
    }

    /// Helper function for grace period logic
    fn should_apply_grace_period(user: &UserData, last_checkin: NaiveDate, today: NaiveDate) -> bool {
        // Grace period only applies to streaks of 30 days or more
        if user.current_streak < 30 {
            return false;
        }

        // Calculate days missed
        let days_missed = today.signed_duration_since(last_checkin).num_days() - 1;

        // Grace period allows up to 2 missed days
        if days_missed <= 2 {
            // Check if we're still within the overall grace period window
            if let Some(grace_start) = user.grace_period_start {
                let grace_days_used = today.signed_duration_since(grace_start).num_days();
                grace_days_used <= 2
            } else {
                // First time using grace period
                true
            }
        } else {
            false
        }
    }
}
