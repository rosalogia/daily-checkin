use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, NaiveDate};

/// Item in the users table (key: guild_id + user_id)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    pub guild_id: u64,
    pub user_id: u64,
    pub goal: String,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub last_checkin_date: Option<NaiveDate>,
    pub grace_period_start: Option<NaiveDate>,
    pub is_active: bool,
    /// Whether replies in the daily thread count as check-ins (set with /thread-checkins).
    /// The check-in button always counts.
    #[serde(default = "default_true")]
    pub thread_checkins: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

/// Item in the guilds table (key: guild_id). The item also holds the current `daily_post`,
/// which is read and written separately (see `Store::get_post`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub guild_id: u64,
    pub checkin_channel_id: Option<u64>,
    pub timezone: String,
    pub daily_time: String,
    /// Scheduled post time of the most recent daily cycle that was claimed, used to make sure
    /// each cycle runs exactly once even if the scheduler fires more than once. Only written by
    /// `Store::claim_cycle`.
    #[serde(default)]
    pub last_cycle_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ServerConfig {
    pub fn new(guild_id: u64) -> Self {
        let now = Utc::now();
        Self {
            guild_id,
            checkin_channel_id: None,
            timezone: "UTC".to_string(),
            daily_time: "09:00".to_string(),
            last_cycle_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A guild's current daily post, stored as the `daily_post` map on its guilds table item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPost {
    /// The day this post represents: the server's local date when it was scheduled. Check-ins
    /// on the post count for this day no matter what time they happen.
    pub cycle_date: NaiveDate,
    pub channel_id: u64,
    pub message_id: u64,
    pub thread_id: Option<u64>,
    pub posted_at: DateTime<Utc>, // When the post was actually created
    pub created_at: DateTime<Utc>,
}
