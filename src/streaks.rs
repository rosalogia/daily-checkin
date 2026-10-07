use crate::{data::{UserData, DailyPost}, store::{Error, Store}};
use chrono::{Utc, NaiveDate, Duration};
use serenity::{
    builder::GetMessages,
    http::Http,
    model::{
        application::ComponentInteraction,
        id::{ChannelId, MessageId, UserId},
    },
};
use std::collections::HashMap;
use tracing::{info, debug, warn};

/// Custom ID of the check-in button attached to each daily post
pub const CHECKIN_BUTTON_ID: &str = "daily-checkin";

/// Result of attempting to record a check-in
pub enum CheckinOutcome {
    CheckedIn { streak: u32 },
    AlreadyCheckedIn,
    /// Thread reply from a user who has switched to button-only check-ins
    UsesButton,
    Inactive,
    NotRegistered,
}

pub struct StreakManager {
    store: Store,
}

impl StreakManager {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// Credit check-ins for replies in a daily post's thread.
    /// Runs before the next daily post, since thread messages are read in a batch rather than live.
    pub async fn process_thread_replies(&self, http: &Http, post: &DailyPost) -> Result<u32, Error> {
        let thread_id: ChannelId = match &post.thread_id {
            Some(id) => id.parse()?,
            None => return Ok(0),
        };
        let deadline = post.posted_at + Duration::hours(24);

        // Find each user's earliest reply within 24 hours of the post
        let mut first_replies: HashMap<UserId, chrono::DateTime<Utc>> = HashMap::new();
        let mut before: Option<MessageId> = None;
        loop {
            let mut request = GetMessages::new().limit(100);
            if let Some(id) = before {
                request = request.before(id);
            }
            let page = thread_id.messages(http, request).await?;

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
            info!("Processing thread check-in from user {} in guild {}", user_id, post.guild_id);
            if let CheckinOutcome::CheckedIn { .. } = self.record_checkin(post, &user_id.to_string(), &message_time, false).await? {
                credited += 1;
            }
        }

        Ok(credited)
    }

    /// Process a click on the daily post's check-in button, returning the ephemeral reply text
    pub async fn process_button(&self, component: &ComponentInteraction) -> Result<String, Error> {
        let guild_id = match component.guild_id {
            Some(id) => id.to_string(),
            None => return Ok("Check-ins only work inside a server.".to_string()),
        };

        // Check if the button is on the current daily post within 24 hours
        let click_time = Utc::now();
        let post = match self.store.get_post(&guild_id).await? {
            Some(post) if post.message_id == component.message.id.to_string()
                && click_time <= post.posted_at + Duration::hours(24) => post,
            _ => return Ok("This check-in post has expired. Use the latest daily post!".to_string()),
        };

        info!("Processing check-in button from user {} in guild {}", component.user.id, guild_id);
        let reply = match self.record_checkin(&post, &component.user.id.to_string(), &click_time, true).await? {
            CheckinOutcome::CheckedIn { streak } => format!("✅ Checked in! Your streak is now 🔥**{}**", streak),
            CheckinOutcome::AlreadyCheckedIn | CheckinOutcome::UsesButton => "You've already checked in today.".to_string(),
            CheckinOutcome::Inactive | CheckinOutcome::NotRegistered => {
                "You're not registered for daily check-ins. Use `/register-goal` to join!".to_string()
            }
        };

        Ok(reply)
    }

    /// Record a check-in against a daily post and update the user's streak
    async fn record_checkin(
        &self,
        post: &DailyPost,
        user_id: &str,
        message_time: &chrono::DateTime<Utc>,
        via_button: bool,
    ) -> Result<CheckinOutcome, Error> {
        let guild_id = &post.guild_id;
        let post_date = post.posted_at.date_naive();
        let response_date = message_time.date_naive();

        // Retry if a concurrent check-in or streak reset modifies the user between read and write
        for _ in 0..3 {
            let mut user = match self.store.get_user(guild_id, user_id).await? {
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

            // Skip thread replies from users who have switched to button-only check-ins
            if !via_button && user.has_used_reaction_checkin {
                debug!("User {} uses button check-ins, ignoring thread replies in guild {}", user_id, guild_id);
                return Ok(CheckinOutcome::UsesButton);
            }

            // If they already checked in on or after the day this post was created, skip
            if user.last_checkin_date.is_some_and(|last_checkin| last_checkin >= post_date) {
                debug!("User {} already checked in for this daily post cycle in guild {}", user_id, guild_id);
                return Ok(CheckinOutcome::AlreadyCheckedIn);
            }

            let read_updated_at = user.updated_at;

            // Mark user as having used button check-in (opts them out of thread-reply check-ins)
            if via_button && !user.has_used_reaction_checkin {
                user.has_used_reaction_checkin = true;
                info!("User {} in guild {} has switched to button-only check-ins", user_id, guild_id);
            }

            // Update user streak
            Self::update_user_streak(&mut user, response_date);
            user.updated_at = Utc::now();

            if self.store.put_user_if_unchanged(guild_id, &user, read_updated_at).await? {
                info!("User {} checked in! New streak: {} days", user_id, user.current_streak);
                return Ok(CheckinOutcome::CheckedIn { streak: user.current_streak });
            }
            debug!("User {} in guild {} changed during check-in, retrying", user_id, guild_id);
        }

        Err(format!("Too many concurrent updates for user {} in guild {}", user_id, guild_id).into())
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

    /// Carry an active streak across a period when the bot couldn't post, by moving the last
    /// check-in to the day before `first_post_date` (the UTC date of the first post after the
    /// outage). Returns whether the user was changed.
    pub fn bridge_outage(user: &mut UserData, first_post_date: NaiveDate) -> bool {
        let bridged_date = match first_post_date.pred_opt() {
            Some(date) => date,
            None => return false,
        };

        if !user.is_active || user.current_streak == 0 {
            return false;
        }
        match user.last_checkin_date {
            Some(last_checkin) if last_checkin < bridged_date => {
                user.last_checkin_date = Some(bridged_date);
                // The outage shouldn't use up anyone's grace period
                user.grace_period_start = None;
                true
            }
            _ => false,
        }
    }

    /// Reset streaks for users in a guild who missed yesterday's check-in
    pub async fn reset_streaks_for_guild(&self, guild_id: &str) -> Result<u32, Error> {
        let yesterday = Utc::now().date_naive().pred_opt().unwrap_or(Utc::now().date_naive());
        let mut reset_count = 0;

        for mut user in self.store.list_users(guild_id).await? {
            if !user.is_active {
                continue;
            }

            // Check if user missed yesterday's check-in
            if let Some(last_checkin) = user.last_checkin_date {
                if last_checkin < yesterday {
                    // User missed check-in, check if grace period applies
                    if !Self::should_apply_grace_period(&user, last_checkin, yesterday.succ_opt().unwrap_or(yesterday)) {
                        // Reset streak
                        let read_updated_at = user.updated_at;
                        user.current_streak = 0;
                        user.grace_period_start = None;
                        user.updated_at = Utc::now();

                        // If the user changed concurrently (e.g. just checked in), leave them alone
                        if self.store.put_user_if_unchanged(guild_id, &user, read_updated_at).await? {
                            reset_count += 1;
                            info!("Reset streak for user {} in guild {} due to missed check-in", user.user_id, guild_id);
                        } else {
                            warn!("User {} in guild {} changed during streak reset, skipping", user.user_id, guild_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn user(current_streak: u32, last_checkin: &str, is_active: bool) -> UserData {
        UserData {
            user_id: "1".to_string(),
            goal: "goal".to_string(),
            current_streak,
            longest_streak: current_streak,
            last_checkin_date: Some(last_checkin.parse().unwrap()),
            grace_period_start: None,
            is_active,
            has_used_reaction_checkin: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn bridged_streak_continues_at_first_post() {
        let mut u = user(9, "2026-10-01", true);
        assert!(StreakManager::bridge_outage(&mut u, date("2026-10-08")));
        assert_eq!(u.last_checkin_date, Some(date("2026-10-07")));

        // Checking in on the first post continues the streak instead of resetting it
        StreakManager::update_user_streak(&mut u, date("2026-10-08"));
        assert_eq!(u.current_streak, 10);
    }

    #[test]
    fn unbridged_streak_resets_after_outage() {
        let mut u = user(9, "2026-10-01", true);
        StreakManager::update_user_streak(&mut u, date("2026-10-08"));
        assert_eq!(u.current_streak, 1);
    }

    #[test]
    fn bridge_skips_broken_and_inactive_streaks() {
        assert!(!StreakManager::bridge_outage(&mut user(0, "2026-03-22", true), date("2026-10-08")));
        assert!(!StreakManager::bridge_outage(&mut user(13, "2026-03-06", false), date("2026-10-08")));
    }

    #[test]
    fn bridge_never_moves_last_checkin_backwards() {
        let mut u = user(14, "2026-10-07", true);
        assert!(!StreakManager::bridge_outage(&mut u, date("2026-10-08")));
        assert_eq!(u.last_checkin_date, Some(date("2026-10-07")));
    }
}
