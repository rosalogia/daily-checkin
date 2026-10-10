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
use std::collections::HashSet;
use tracing::{info, debug, warn};

/// Custom ID of the check-in button attached to each daily post
pub const CHECKIN_BUTTON_ID: &str = "daily-checkin";

/// Result of attempting to record a check-in
pub enum CheckinOutcome {
    CheckedIn { streak: u32 },
    AlreadyCheckedIn,
    /// Thread reply from a user who has turned off thread check-ins
    ThreadCheckinsDisabled,
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
    pub async fn process_thread_replies(&self, http: &Http, guild_id: u64, post: &DailyPost) -> Result<u32, Error> {
        let thread_id = match post.thread_id {
            Some(id) => ChannelId::new(id),
            None => return Ok(0),
        };
        let deadline = post.posted_at + Duration::hours(24);

        // Find users who replied within 24 hours of the post
        let mut repliers: HashSet<UserId> = HashSet::new();
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
                repliers.insert(msg.author.id);
            }

            // Messages come back newest first; stop once a page isn't full
            match page.last() {
                Some(oldest) if page.len() == 100 => before = Some(oldest.id),
                _ => break,
            }
        }

        let mut credited = 0;
        for user_id in repliers {
            info!("Processing thread check-in from user {} in guild {}", user_id, guild_id);
            if let CheckinOutcome::CheckedIn { .. } = self.record_checkin(guild_id, post, user_id.get(), false).await? {
                credited += 1;
            }
        }

        Ok(credited)
    }

    /// Process a click on the daily post's check-in button, returning the ephemeral reply text
    pub async fn process_button(&self, component: &ComponentInteraction) -> Result<String, Error> {
        let guild_id = match component.guild_id {
            Some(id) => id.get(),
            None => return Ok("Check-ins only work inside a server.".to_string()),
        };

        // Check if the button is on the current daily post within 24 hours
        let click_time = Utc::now();
        let post = match self.store.get_post(guild_id).await? {
            Some(post) if post.message_id == component.message.id.get()
                && click_time <= post.posted_at + Duration::hours(24) => post,
            _ => return Ok("This check-in post has expired. Use the latest daily post!".to_string()),
        };

        info!("Processing check-in button from user {} in guild {}", component.user.id, guild_id);
        let reply = match self.record_checkin(guild_id, &post, component.user.id.get(), true).await? {
            CheckinOutcome::CheckedIn { streak } => format!("✅ Checked in! Your streak is now 🔥**{}**", streak),
            // ThreadCheckinsDisabled only applies to thread replies, never the button
            CheckinOutcome::AlreadyCheckedIn | CheckinOutcome::ThreadCheckinsDisabled => "You've already checked in today.".to_string(),
            CheckinOutcome::Inactive | CheckinOutcome::NotRegistered => {
                "You're not registered for daily check-ins. Use `/register-goal` to join!".to_string()
            }
        };

        Ok(reply)
    }

    /// Record a check-in against a daily post and update the user's streak
    async fn record_checkin(
        &self,
        guild_id: u64,
        post: &DailyPost,
        user_id: u64,
        via_button: bool,
    ) -> Result<CheckinOutcome, Error> {
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

            let read_updated_at = user.updated_at;
            let outcome = Self::apply_checkin(&mut user, post.cycle_date, via_button);
            match outcome {
                CheckinOutcome::CheckedIn { .. } => {}
                CheckinOutcome::ThreadCheckinsDisabled => {
                    debug!("User {} has thread check-ins off, ignoring thread replies in guild {}", user_id, guild_id);
                    return Ok(outcome);
                }
                _ => {
                    debug!("User {} already checked in for this daily post cycle in guild {}", user_id, guild_id);
                    return Ok(outcome);
                }
            }

            user.updated_at = Utc::now();
            if self.store.put_user_if_unchanged(&user, read_updated_at).await? {
                info!("User {} checked in! New streak: {} days", user_id, user.current_streak);
                return Ok(outcome);
            }
            debug!("User {} in guild {} changed during check-in, retrying", user_id, guild_id);
        }

        Err(format!("Too many concurrent updates for user {} in guild {}", user_id, guild_id).into())
    }

    /// Apply an active user's check-in on the post for `cycle_date`, updating their streak.
    /// The check-in counts for the post's day regardless of when it happened.
    fn apply_checkin(user: &mut UserData, cycle_date: NaiveDate, via_button: bool) -> CheckinOutcome {
        // Skip thread replies from users who have turned off thread check-ins
        if !via_button && !user.thread_checkins {
            return CheckinOutcome::ThreadCheckinsDisabled;
        }

        // Skip if they already checked in for this post's day (or a later one)
        if user.last_checkin_date.is_some_and(|last_checkin| last_checkin >= cycle_date) {
            return CheckinOutcome::AlreadyCheckedIn;
        }

        Self::update_user_streak(user, cycle_date);
        CheckinOutcome::CheckedIn { streak: user.current_streak }
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
    /// check-in to the day before `first_cycle_date` (the cycle date of the first post after the
    /// outage). Returns whether the user was changed.
    pub fn bridge_outage(user: &mut UserData, first_cycle_date: NaiveDate) -> bool {
        let bridged_date = match first_cycle_date.pred_opt() {
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

    /// Reset streaks for users in a guild who missed the previous post, run before the post
    /// for `cycle_date` goes out
    pub async fn reset_streaks_for_guild(&self, guild_id: u64, cycle_date: NaiveDate) -> Result<u32, Error> {
        let mut reset_count = 0;

        for mut user in self.store.list_users(guild_id).await? {
            if !Self::missed_previous_cycle(&user, cycle_date) {
                continue;
            }

            // Reset streak
            let read_updated_at = user.updated_at;
            user.current_streak = 0;
            user.grace_period_start = None;
            user.updated_at = Utc::now();

            // If the user changed concurrently (e.g. just checked in), leave them alone
            if self.store.put_user_if_unchanged(&user, read_updated_at).await? {
                reset_count += 1;
                info!("Reset streak for user {} in guild {} due to missed check-in", user.user_id, guild_id);
            } else {
                warn!("User {} in guild {} changed during streak reset, skipping", user.user_id, guild_id);
            }
        }

        Ok(reset_count)
    }

    /// Whether an active user missed the post before `cycle_date` and isn't covered by the
    /// grace period, so their streak should reset
    fn missed_previous_cycle(user: &UserData, cycle_date: NaiveDate) -> bool {
        let previous_cycle = match cycle_date.pred_opt() {
            Some(date) => date,
            None => return false,
        };

        match user.last_checkin_date {
            Some(last_checkin) if user.is_active && last_checkin < previous_cycle => {
                !Self::should_apply_grace_period(user, last_checkin, cycle_date)
            }
            _ => false,
        }
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
            guild_id: 1,
            user_id: 1,
            goal: "goal".to_string(),
            current_streak,
            longest_streak: current_streak,
            last_checkin_date: Some(last_checkin.parse().unwrap()),
            grace_period_start: None,
            is_active,
            thread_checkins: true,
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
    fn checkins_count_for_the_post_day() {
        // One check-in per post, whatever time of day it happens: each post's cycle date
        // decides the day, so consecutive posts extend the streak
        let mut u = user(5, "2026-10-07", true);
        assert!(matches!(StreakManager::apply_checkin(&mut u, date("2026-10-08"), true), CheckinOutcome::CheckedIn { streak: 6 }));
        assert!(matches!(StreakManager::apply_checkin(&mut u, date("2026-10-09"), false), CheckinOutcome::CheckedIn { streak: 7 }));
        assert_eq!(u.last_checkin_date, Some(date("2026-10-09")));
    }

    #[test]
    fn second_checkin_on_same_post_is_ignored() {
        let mut u = user(5, "2026-10-08", true);
        assert!(matches!(StreakManager::apply_checkin(&mut u, date("2026-10-08"), true), CheckinOutcome::AlreadyCheckedIn));
        assert_eq!(u.current_streak, 5);
    }

    #[test]
    fn thread_replies_ignored_when_disabled_but_button_counts() {
        let mut u = user(5, "2026-10-07", true);
        u.thread_checkins = false;
        assert!(matches!(StreakManager::apply_checkin(&mut u, date("2026-10-08"), false), CheckinOutcome::ThreadCheckinsDisabled));
        assert!(matches!(StreakManager::apply_checkin(&mut u, date("2026-10-08"), true), CheckinOutcome::CheckedIn { streak: 6 }));
    }

    #[test]
    fn reset_only_after_missing_the_previous_post() {
        // Before the 10/10 post: checked in on the 10/9 post, safe
        assert!(!StreakManager::missed_previous_cycle(&user(5, "2026-10-09", true), date("2026-10-10")));
        // Last checked in on the 10/8 post, so missed 10/9
        assert!(StreakManager::missed_previous_cycle(&user(5, "2026-10-08", true), date("2026-10-10")));
        // 30+ day streaks get a grace period for the miss
        assert!(!StreakManager::missed_previous_cycle(&user(45, "2026-10-08", true), date("2026-10-10")));
        // Inactive users are left alone
        assert!(!StreakManager::missed_previous_cycle(&user(5, "2026-10-01", false), date("2026-10-10")));
    }

    #[test]
    fn thread_checkins_default_on_for_existing_users() {
        // Users saved before the setting existed, including those auto-opted-out by the old
        // reaction/button flag, count thread replies again
        let json = r#"{"guild_id":1,"user_id":1,"goal":"g","current_streak":3,"longest_streak":3,
            "last_checkin_date":"2026-10-06","grace_period_start":null,"is_active":true,
            "has_used_reaction_checkin":true,
            "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let user: UserData = serde_json::from_str(json).unwrap();
        assert!(user.thread_checkins);
    }

    #[test]
    fn bridge_never_moves_last_checkin_backwards() {
        let mut u = user(14, "2026-10-07", true);
        assert!(!StreakManager::bridge_outage(&mut u, date("2026-10-08")));
        assert_eq!(u.last_checkin_date, Some(date("2026-10-07")));
    }
}
