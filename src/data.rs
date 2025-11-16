use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc, NaiveDate};
use anyhow::Result;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    pub user_id: String,
    pub goal: String,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub last_checkin_date: Option<NaiveDate>,
    pub grace_period_start: Option<NaiveDate>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub guild_id: String,
    pub checkin_channel_id: Option<String>,
    pub timezone: String,
    pub daily_time: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinRecord {
    pub user_id: String,
    pub checkin_date: NaiveDate,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub daily_post_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPost {
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub thread_id: Option<String>,
    pub post_date: NaiveDate,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BotData {
    pub servers: HashMap<String, ServerConfig>,
    pub users: HashMap<String, HashMap<String, UserData>>, // guild_id -> user_id -> UserData
    pub checkins: HashMap<String, Vec<CheckinRecord>>, // guild_id -> checkins
    pub daily_posts: HashMap<String, Vec<DailyPost>>, // guild_id -> posts
}

impl BotData {
    fn data_file_path() -> String {
        std::env::var("DATA_FILE_PATH").unwrap_or_else(|_| "bot_data.json".to_string())
    }

    pub async fn load() -> Result<Self> {
        let file_path = Self::data_file_path();
        match fs::read_to_string(&file_path).await {
            Ok(content) => {
                let data: BotData = serde_json::from_str(&content)?;
                Ok(data)
            }
            Err(_) => {
                // File doesn't exist, return default
                Ok(BotData::default())
            }
        }
    }

    pub async fn save(&self) -> Result<()> {
        let file_path = Self::data_file_path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&file_path, content).await?;
        Ok(())
    }

    pub fn get_user(&self, guild_id: &str, user_id: &str) -> Option<&UserData> {
        self.users.get(guild_id)?.get(user_id)
    }

    pub fn get_server_config(&self, guild_id: &str) -> Option<&ServerConfig> {
        self.servers.get(guild_id)
    }

    pub fn add_or_update_user(&mut self, guild_id: String, user_data: UserData) {
        self.users
            .entry(guild_id)
            .or_insert_with(HashMap::new)
            .insert(user_data.user_id.clone(), user_data);
    }

    pub fn add_or_update_server(&mut self, server_config: ServerConfig) {
        self.servers.insert(server_config.guild_id.clone(), server_config);
    }
}