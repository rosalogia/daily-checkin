use crate::{
    data::{DailyPost, ServerConfig, UserData},
    store::{Error, Store},
    streaks::{StreakManager, CHECKIN_BUTTON_ID},
};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serenity::{
    builder::{CreateActionRow, CreateButton, CreateEmbed, CreateMessage, CreateThread, EditThread},
    http::Http,
    model::{application::ButtonStyle, channel::ChannelType, id::{ChannelId, MessageId}},
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

/// Payload for the daily function
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DailyEvent {
    /// Sent by the scheduler every minute; runs cycles for servers whose post time has arrived
    Scheduled,
    /// Sent by /trigger-checkin; runs one server's cycle immediately and reports back by editing
    /// the deferred interaction response
    Manual {
        guild_id: u64,
        channel_id: u64,
        application_id: u64,
        interaction_token: String,
    },
}

/// How long after the configured time a missed cycle may still run (e.g. if a scheduler
/// invocation was delayed or failed)
const CATCH_UP_WINDOW_MINUTES: i64 = 15;

/// The UTC instant of a server's configured post time on a local date in its timezone.
/// Returns None on days where that local time doesn't exist (DST transitions).
pub fn scheduled_at(config: &ServerConfig, local_date: NaiveDate) -> Result<Option<DateTime<Utc>>, Error> {
    let target_time = NaiveTime::parse_from_str(&config.daily_time, "%H:%M")?;
    let tz: Tz = config.timezone.parse()?;

    // Pick the earliest valid mapping when the time is ambiguous
    Ok(tz.from_local_datetime(&local_date.and_time(target_time)).earliest().map(|local| local.with_timezone(&Utc)))
}

/// The server's local date at `instant`, which is the cycle date of a post made then
pub fn local_date(config: &ServerConfig, instant: DateTime<Utc>) -> Result<NaiveDate, Error> {
    let tz: Tz = config.timezone.parse()?;
    Ok(instant.with_timezone(&tz).date_naive())
}

/// The UTC instant of today's configured post time for a server, if it falls within the
/// catch-up window ending at `now`
pub fn due_cycle(config: &ServerConfig, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, Error> {
    let tz: Tz = config.timezone.parse()?;
    let local_date = now.with_timezone(&tz).date_naive();

    let scheduled_at = match scheduled_at(config, local_date)? {
        Some(at) => at,
        None => return Ok(None),
    };

    let window = now.signed_duration_since(scheduled_at);
    if window < Duration::zero() || window >= Duration::minutes(CATCH_UP_WINDOW_MINUTES) {
        return Ok(None);
    }

    // Skip if this cycle already ran
    if config.last_cycle_at.is_some_and(|last| last >= scheduled_at) {
        return Ok(None);
    }

    Ok(Some(scheduled_at))
}

/// Run the daily cycle for every server whose post time has arrived
pub async fn run_due_cycles(store: &Store, http: &Http) -> Result<(), Error> {
    let now = Utc::now();

    for config in store.list_configs().await? {
        let guild_id = config.guild_id;

        // Skip if no channel configured
        let channel_id = match config.checkin_channel_id {
            Some(id) => ChannelId::new(id),
            None => {
                debug!("No checkin channel configured for guild {}", guild_id);
                continue;
            }
        };

        let scheduled_at = match due_cycle(&config, now) {
            Ok(Some(at)) => at,
            Ok(None) => continue,
            Err(e) => {
                error!("Invalid schedule for guild {}: {}", guild_id, e);
                continue;
            }
        };

        // Claim the cycle so duplicate scheduler invocations don't post twice
        if !store.claim_cycle(guild_id, scheduled_at).await? {
            debug!("Cycle at {} already claimed for guild {}", scheduled_at, guild_id);
            continue;
        }

        info!("Posting daily message for guild {} in channel {}", guild_id, channel_id);
        let cycle_date = local_date(&config, scheduled_at)?;
        if let Err(e) = run_cycle(store, http, guild_id, channel_id, cycle_date).await {
            error!("Daily cycle failed for guild {}: {}", guild_id, e);
        }
    }

    Ok(())
}

/// Credit thread replies to the previous post, reset missed streaks, then post the new daily
/// message for `cycle_date`
pub async fn run_cycle(store: &Store, http: &Http, guild_id: u64, channel_id: ChannelId, cycle_date: NaiveDate) -> Result<(), Error> {
    let streak_manager = StreakManager::new(store.clone());
    let previous_post = store.get_post(guild_id).await?;

    // Credit thread replies to the previous post before streak maintenance
    if let Some(post) = &previous_post {
        match streak_manager.process_thread_replies(http, guild_id, post).await {
            Ok(count) => info!("Credited {} thread check-ins for guild {}", count, guild_id),
            Err(e) => error!("Failed to process thread replies for guild {}: {}", guild_id, e),
        }
    }

    // Run streak maintenance
    match streak_manager.reset_streaks_for_guild(guild_id, cycle_date).await {
        Ok(reset_count) => {
            if reset_count > 0 {
                info!("Reset {} streaks for guild {} before daily post", reset_count, guild_id);
            }
        }
        Err(e) => {
            error!("Failed to run streak maintenance for guild {}: {}", guild_id, e);
        }
    }

    // Archive the previous daily post before creating a new one
    if let Some(post) = &previous_post {
        if let Err(e) = archive_post(http, guild_id, post).await {
            error!("Failed to archive previous post for guild {}: {}", guild_id, e);
        }
    }

    post_daily_message(store, http, guild_id, channel_id, cycle_date).await
}

/// Archive a daily post (archive thread + delete message)
async fn archive_post(http: &Http, guild_id: u64, post: &DailyPost) -> Result<(), Error> {
    // Archive the thread first (if one exists)
    if let Some(thread_id) = post.thread_id.map(ChannelId::new) {
        if let Err(e) = thread_id.edit_thread(http, EditThread::new().archived(true)).await {
            error!("Failed to archive thread {} for guild {}: {}", thread_id, guild_id, e);
        }
    }

    // Delete the main embed message to clear channel clutter
    let channel_id = ChannelId::new(post.channel_id);
    let message_id = MessageId::new(post.message_id);
    if let Err(e) = channel_id.delete_message(http, message_id).await {
        error!("Failed to delete previous daily post for guild {}: {}", guild_id, e);
    }

    info!("Archived previous daily post for guild {}", guild_id);
    Ok(())
}

/// Post the daily check-in message
async fn post_daily_message(store: &Store, http: &Http, guild_id: u64, channel_id: ChannelId, cycle_date: NaiveDate) -> Result<(), Error> {
    // Active users, sorted by streak (highest first) for motivation
    let mut active_users: Vec<UserData> = store.list_users(guild_id).await?
        .into_iter()
        .filter(|user| user.is_active)
        .collect();
    active_users.sort_by(|a, b| b.current_streak.cmp(&a.current_streak));

    // Post the message with a check-in button
    let checkin_button = CreateButton::new(CHECKIN_BUTTON_ID)
        .label("Check in")
        .emoji('🔥')
        .style(ButtonStyle::Success);
    let message = channel_id.send_message(http,
        CreateMessage::new()
            .add_embed(generate_daily_embed(&active_users))
            .components(vec![CreateActionRow::Buttons(vec![checkin_button])])
    ).await?;

    // Create a thread named for the post's day
    let thread_name = format!("Daily Check-in Responses {}", cycle_date.format("%m/%d/%y"));
    let thread = message
        .channel_id
        .create_thread(http, CreateThread::new(thread_name).kind(ChannelType::PublicThread))
        .await?;

    // Send a ping message in the thread to notify all participants
    send_thread_pings(http, thread.id, &active_users).await?;

    // Save the daily post record
    let now = Utc::now();
    let daily_post = DailyPost {
        cycle_date,
        channel_id: channel_id.get(),
        message_id: message.id.get(),
        thread_id: Some(thread.id.get()),
        posted_at: now, // When the post was actually created
        created_at: now,
    };
    store.put_post(guild_id, &daily_post).await?;

    info!("Successfully posted daily message for guild {} with thread {}", guild_id, thread.id);
    Ok(())
}

/// Generate the daily message embed with user pings, goals, and streaks
fn generate_daily_embed(active_users: &[UserData]) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title("Daily Check-in")
        .description("Press 🔥 **Check in** below OR reply to the thread below to check in!")
        .color(0x00ff88); // Green color for daily check-ins

    if active_users.is_empty() {
        return embed.field("No Users Registered", "Use `/register-goal` to join!", false);
    }

    // Build user list for the field
    let mut user_list = String::new();
    for user in active_users {
        let user_mention = format!("<@{}>", user.user_id);

        // Truncate goal if it's too long for readability
        let goal_display = if user.goal.chars().count() > 50 {
            format!("{}...", user.goal.chars().take(47).collect::<String>())
        } else {
            user.goal.clone()
        };

        user_list.push_str(&format!("• {} - **{}** 🔥**{}**\n", user_mention, goal_display, user.current_streak));
    }

    embed = embed.field("Participants", user_list, false);
    embed
}

/// Send ping message to thread to notify all participants
async fn send_thread_pings(http: &Http, thread_id: ChannelId, active_users: &[UserData]) -> Result<(), Error> {
    if !active_users.is_empty() {
        let mentions: Vec<String> = active_users
            .iter()
            .map(|user| format!("<@{}>", user.user_id))
            .collect();

        let ping_message = format!("Time to check in!\n{}", mentions.join("\n"));

        // Send the ping message to the thread
        thread_id.send_message(http, CreateMessage::new().content(ping_message)).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(daily_time: &str, timezone: &str, last_cycle_at: Option<DateTime<Utc>>) -> ServerConfig {
        ServerConfig {
            daily_time: daily_time.to_string(),
            timezone: timezone.to_string(),
            last_cycle_at,
            ..ServerConfig::new(1)
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn due_at_and_shortly_after_scheduled_time() {
        let c = config("09:00", "America/New_York", None);
        // 09:00 EDT is 13:00 UTC
        assert_eq!(due_cycle(&c, utc("2026-10-06T13:00:05Z")).unwrap(), Some(utc("2026-10-06T13:00:00Z")));
        assert_eq!(due_cycle(&c, utc("2026-10-06T13:14:59Z")).unwrap(), Some(utc("2026-10-06T13:00:00Z")));
    }

    #[test]
    fn not_due_outside_window() {
        let c = config("09:00", "America/New_York", None);
        assert_eq!(due_cycle(&c, utc("2026-10-06T12:59:59Z")).unwrap(), None);
        assert_eq!(due_cycle(&c, utc("2026-10-06T13:15:00Z")).unwrap(), None);
    }

    #[test]
    fn not_due_when_already_claimed() {
        let c = config("09:00", "UTC", Some(utc("2026-10-06T09:00:00Z")));
        assert_eq!(due_cycle(&c, utc("2026-10-06T09:01:00Z")).unwrap(), None);

        // Yesterday's claim doesn't block today
        let c = config("09:00", "UTC", Some(utc("2026-10-05T09:00:00Z")));
        assert_eq!(due_cycle(&c, utc("2026-10-06T09:01:00Z")).unwrap(), Some(utc("2026-10-06T09:00:00Z")));
    }

    #[test]
    fn cycle_date_is_the_servers_local_date() {
        // A 21:00 New York post goes out at 01:00 UTC the next day, but belongs to the New
        // York date. A check-in between 20:00 and 21:00 New York the following evening (already
        // the day after that in UTC) still counts for the same post.
        let c = config("21:00", "America/New_York", None);
        let posted = scheduled_at(&c, date("2026-10-08")).unwrap().unwrap();
        assert_eq!(posted, utc("2026-10-09T01:00:00Z"));
        assert_eq!(local_date(&c, posted).unwrap(), date("2026-10-08"));
    }

    #[test]
    fn cycle_dates_stay_consecutive_across_dst() {
        // At 19:30 New York, posts move from 23:30 UTC to 00:30 UTC when DST ends (Nov 1,
        // 2026). UTC dates would skip a day (10/31 -> 11/2); local dates don't.
        let c = config("19:30", "America/New_York", None);
        let before = scheduled_at(&c, date("2026-10-31")).unwrap().unwrap();
        let after = scheduled_at(&c, date("2026-11-01")).unwrap().unwrap();
        assert_eq!((before.date_naive(), after.date_naive()), (date("2026-10-31"), date("2026-11-02")));
        assert_eq!(local_date(&c, before).unwrap(), date("2026-10-31"));
        assert_eq!(local_date(&c, after).unwrap(), date("2026-11-01"));
    }

    #[test]
    fn uses_local_date_near_midnight() {
        // 23:30 in Tokyo on Oct 6 is 14:30 UTC on Oct 6
        let c = config("23:30", "Asia/Tokyo", None);
        assert_eq!(due_cycle(&c, utc("2026-10-06T14:31:00Z")).unwrap(), Some(utc("2026-10-06T14:30:00Z")));
    }
}
