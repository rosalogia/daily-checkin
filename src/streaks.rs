use crate::{bot::SharedBotData, data::{UserData, BotData, DailyPost}};
use chrono::{Utc, NaiveDate, Duration};
use serenity::{
    builder::GetMessages,
    model::{
        application::ComponentInteraction,
        id::{GuildId, ChannelId, MessageId, UserId},
    },
    prelude::Context,
};
use std::collections::HashMap;
use tracing::{info, debug, error};

/// Custom ID of the check-in button attached to each daily post
pub const CHECKIN_BUTTON_ID: &str = "daily-checkin";

/// Result of attempting to record a check-in
pub enum CheckinOutcome {
    CheckedIn { streak: u32 },
    AlreadyCheckedIn,
    Inactive,
    NotRegistered,
}

pub struct StreakManager {
    data: SharedBotData,
}

impl StreakManager {
    pub fn new(data: SharedBotData) -> Self {
        Self { data }
    }

    /// Credit check-ins for replies in the current daily post's thread.
    /// Runs before the next daily post, since thread messages are read in a batch rather than live.
    pub async fn process_thread_replies(&self, ctx: &Context, guild_id: GuildId) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let (thread_id, deadline) = {
            let data = self.data.read().await;
            match data.daily_posts.get(&guild_id.to_string()) {
                Some(DailyPost { thread_id: Some(thread_id), posted_at, .. }) => {
                    (thread_id.parse::<ChannelId>()?, *posted_at + Duration::hours(24))
                }
                _ => return Ok(0),
            }
        };

        // Find each user's earliest reply within 24 hours of the post
        let mut first_replies: HashMap<UserId, chrono::DateTime<Utc>> = HashMap::new();
        let mut before: Option<MessageId> = None;
        loop {
            let mut request = GetMessages::new().limit(100);
            if let Some(id) = before {
                request = request.before(id);
            }
            let page = thread_id.messages(&ctx.http, request).await?;

            for msg in &page {
                if msg.author.bot {
                    continue;
                }
                let message_time = chrono::DateTime::<Utc>::from_timestamp(msg.timestamp.unix_timestamp(), 0)
                    .unwrap_or_else(|| Utc::now());
                if message_time > deadline {
                    continue;
                }
                first_replies
                    .entry(msg.author.id)
                    .and_modify(|t| *t = (*t).min(message_time))
                    .or_insert(message_time);
            }

            // Messages come back newest first; stop once a page isn't full
            match page.last() {
                Some(oldest) if page.len() == 100 => before = Some(oldest.id),
                _ => break,
            }
        }

        let mut credited = 0;
        for (user_id, message_time) in first_replies {
            // Skip users who have switched to button-only check-ins
            {
                let data = self.data.read().await;
                if let Some(user) = data.get_user(&guild_id.to_string(), &user_id.to_string()) {
                    if user.has_used_reaction_checkin {
                        debug!("User {} uses button check-ins, ignoring thread replies in guild {}", user_id, guild_id);
                        continue;
                    }
                }
            }

            info!("Processing thread check-in from user {} in guild {}", user_id, guild_id);
            if let CheckinOutcome::CheckedIn { .. } = self.record_checkin(guild_id, user_id, &message_time, false).await? {
                credited += 1;
            }
        }

        Ok(credited)
    }

    /// Process a click on the daily post's check-in button, returning the ephemeral reply text
    pub async fn process_button(&self, component: &ComponentInteraction) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = match component.guild_id {
            Some(id) => id,
            None => return Ok("Check-ins only work inside a server.".to_string()),
        };

        // Check if the button is on the current daily post within 24 hours
        let click_time = Utc::now();
        if !self.is_valid_checkin_post(guild_id, component.message.id, &click_time).await {
            return Ok("This check-in post has expired. Use the latest daily post!".to_string());
        }

        info!("Processing check-in button from user {} in guild {}", component.user.id, guild_id);
        let reply = match self.record_checkin(guild_id, component.user.id, &click_time, true).await? {
            CheckinOutcome::CheckedIn { streak } => format!("✅ Checked in! Your streak is now 🔥**{}**", streak),
            CheckinOutcome::AlreadyCheckedIn => "You've already checked in today.".to_string(),
            CheckinOutcome::Inactive | CheckinOutcome::NotRegistered => {
                "You're not registered for daily check-ins. Use `/register-goal` to join!".to_string()
            }
        };

        Ok(reply)
    }

    /// Check if a message is the current daily check-in post and still within 24 hours
    async fn is_valid_checkin_post(&self, guild_id: GuildId, message_id: MessageId, click_time: &chrono::DateTime<Utc>) -> bool {
        let data = self.data.read().await;
        let guild_id_str = guild_id.to_string();
        let message_id_str = message_id.to_string();

        if let Some(daily_post) = data.daily_posts.get(&guild_id_str) {
            // Check if this is the current daily post message
            if &message_id_str == &daily_post.message_id {
                // Calculate 24-hour deadline: daily post time + 24 hours
                let deadline = daily_post.posted_at + Duration::hours(24);

                // Check if the button was clicked before the deadline
                return *click_time <= deadline;
            }
        }

        false
    }

    /// Record a check-in and update user streak
    async fn record_checkin(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        message_time: &chrono::DateTime<Utc>,
        via_button: bool,
    ) -> Result<CheckinOutcome, Box<dyn std::error::Error + Send + Sync>> {
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
                return Ok(CheckinOutcome::Inactive);
            }
            None => {
                debug!("User {} not registered in guild {}, ignoring check-in", user_id, guild_id);
                return Ok(CheckinOutcome::NotRegistered);
            }
        };
        
        if let Some(post_date) = post_date {
            if let Some(last_checkin) = user.last_checkin_date {
                // If they already checked in on or after the day this post was created, skip
                if last_checkin >= post_date {
                    debug!("User {} already checked in for this daily post cycle in guild {}", user_id, guild_id);
                    return Ok(CheckinOutcome::AlreadyCheckedIn);
                }
            }
        }

        // Mark user as having used button check-in (opts them out of thread-message check-ins).
        // The field keeps its original name so existing data files still deserialize.
        if via_button && !user.has_used_reaction_checkin {
            user.has_used_reaction_checkin = true;
            info!("User {} in guild {} has switched to button-only check-ins", user_id, guild_id);
        }

        // Update user streak
        Self::update_user_streak(user, response_date);
        info!("User {} checked in! New streak: {} days", user_id, user.current_streak);
        let streak = user.current_streak;

        // Save data
        if let Err(e) = data.save().await {
            error!("Failed to save data after recording check-in: {}", e);
            return Err(e.into());
        }

        Ok(CheckinOutcome::CheckedIn { streak })
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
