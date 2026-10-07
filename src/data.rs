use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, NaiveDate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    pub user_id: String,
    pub goal: String,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub last_checkin_date: Option<NaiveDate>,
    pub grace_period_start: Option<NaiveDate>,
    pub is_active: bool,
    /// Set once a user checks in with the button (originally a reaction); opts them out of
    /// thread-reply check-ins. Keeps its original name so imported data still deserializes.
    #[serde(default)]
    pub has_used_reaction_checkin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub guild_id: String,
    pub checkin_channel_id: Option<String>,
    pub timezone: String,
    pub daily_time: String,
    /// Scheduled post time of the most recent daily cycle that was claimed, used to make sure
    /// each cycle runs exactly once even if the scheduler fires more than once
    #[serde(default)]
    pub last_cycle_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ServerConfig {
    pub fn new(guild_id: String) -> Self {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPost {
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub thread_id: Option<String>,
    pub posted_at: DateTime<Utc>, // When the post was actually created
    pub created_at: DateTime<Utc>,
}
