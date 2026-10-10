//! One-time import of the VPS bot's JSON data file into DynamoDB:
//!
//!     GUILDS_TABLE=... USERS_TABLE=... cargo run --bin import-data -- bot_data.json [--resume-on YYYY-MM-DD] [--dry-run]
//!
//! --resume-on  Local date (in each server's timezone) of the first daily post the Lambda will
//!              make. Active streaks are carried across the gap since the old bot's last post.
//! --dry-run    Print what would be imported without writing to DynamoDB.

use chrono::{DateTime, NaiveDate, Utc};
use daily_checkin_bot::{
    daily,
    data::{DailyPost, ServerConfig, UserData},
    store::{Error, Store},
    streaks::StreakManager,
};
use serde::Deserialize;
use std::collections::HashMap;

/// Shape of the old bot_data.json file, which stored IDs as strings
#[derive(Deserialize)]
struct LegacyBotData {
    servers: HashMap<String, LegacyServerConfig>,
    users: HashMap<String, HashMap<String, LegacyUserData>>, // guild_id -> user_id -> user
    daily_posts: HashMap<String, LegacyDailyPost>, // guild_id -> current post
}

#[derive(Deserialize)]
struct LegacyServerConfig {
    guild_id: String,
    checkin_channel_id: Option<String>,
    timezone: String,
    daily_time: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct LegacyUserData {
    user_id: String,
    goal: String,
    current_streak: u32,
    longest_streak: u32,
    last_checkin_date: Option<NaiveDate>,
    grace_period_start: Option<NaiveDate>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct LegacyDailyPost {
    channel_id: String,
    message_id: String,
    thread_id: Option<String>,
    posted_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

fn parse_id(id: &str) -> Result<u64, Error> {
    id.parse().map_err(|_| format!("Invalid ID: {}", id).into())
}

/// Convert the legacy data, keyed by guild ID
fn convert(data: LegacyBotData) -> Result<(Vec<ServerConfig>, Vec<UserData>, Vec<(u64, DailyPost)>), Error> {
    let mut configs = Vec::new();
    for legacy in data.servers.into_values() {
        configs.push(ServerConfig {
            guild_id: parse_id(&legacy.guild_id)?,
            checkin_channel_id: legacy.checkin_channel_id.as_deref().map(parse_id).transpose()?,
            timezone: legacy.timezone,
            daily_time: legacy.daily_time,
            last_cycle_at: None,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
        });
    }

    let mut users = Vec::new();
    for (guild_id, guild_users) in data.users {
        let guild_id = parse_id(&guild_id)?;
        for legacy in guild_users.into_values() {
            users.push(UserData {
                guild_id,
                user_id: parse_id(&legacy.user_id)?,
                goal: legacy.goal,
                current_streak: legacy.current_streak,
                longest_streak: legacy.longest_streak,
                last_checkin_date: legacy.last_checkin_date,
                grace_period_start: legacy.grace_period_start,
                is_active: legacy.is_active,
                // Thread replies count for everyone by default (see /thread-checkins)
                thread_checkins: true,
                created_at: legacy.created_at,
                updated_at: legacy.updated_at,
            });
        }
    }

    let mut posts = Vec::new();
    for (guild_id, legacy) in data.daily_posts {
        let guild_id = parse_id(&guild_id)?;
        let config = configs.iter().find(|c| c.guild_id == guild_id)
            .ok_or_else(|| format!("Daily post for guild {} has no server config", guild_id))?;
        posts.push((guild_id, DailyPost {
            cycle_date: daily::local_date(config, legacy.posted_at)?,
            channel_id: parse_id(&legacy.channel_id)?,
            message_id: parse_id(&legacy.message_id)?,
            thread_id: legacy.thread_id.as_deref().map(parse_id).transpose()?,
            posted_at: legacy.posted_at,
            created_at: legacy.created_at,
        }));
    }

    Ok((configs, users, posts))
}

const USAGE: &str = "usage: import-data <path to bot_data.json> [--resume-on YYYY-MM-DD] [--dry-run]";

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    let mut path = None;
    let mut resume_on: Option<NaiveDate> = None;
    let mut dry_run = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--resume-on" => resume_on = Some(args.next().ok_or(USAGE)?.parse().map_err(|_| "--resume-on expects YYYY-MM-DD")?),
            "--dry-run" => dry_run = true,
            _ if path.is_none() && !arg.starts_with("--") => path = Some(arg),
            _ => return Err(USAGE.into()),
        }
    }
    let path = path.ok_or(USAGE)?;

    let legacy: LegacyBotData = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let (configs, mut users, posts) = convert(legacy)?;

    if let Some(local_date) = resume_on {
        for config in &configs {
            let first_post = daily::scheduled_at(config, local_date)?
                .ok_or_else(|| format!("{} {} doesn't exist in {}", local_date, config.daily_time, config.timezone))?;
            println!("Guild {}: first post at {}", config.guild_id, first_post);

            for user in users.iter_mut().filter(|user| user.guild_id == config.guild_id) {
                let last_checkin = user.last_checkin_date;
                if StreakManager::bridge_outage(user, local_date) {
                    println!(
                        "  Carried streak {} for user {} (last check-in {} -> {})",
                        user.current_streak,
                        user.user_id,
                        last_checkin.map_or("none".to_string(), |d| d.to_string()),
                        user.last_checkin_date.map_or("none".to_string(), |d| d.to_string()),
                    );
                }
            }
        }
    }

    if dry_run {
        println!("Dry run: would import {} server configs, {} users, {} daily posts",
            configs.len(), users.len(), posts.len());
        return Ok(());
    }

    let store = Store::from_env().await?;

    for config in &configs {
        store.save_config(config).await?;
    }
    println!("Imported {} server configs", configs.len());

    for user in &users {
        store.put_user(user).await?;
    }
    println!("Imported {} users", users.len());

    // Configs are saved first so each post's guild item exists
    for (guild_id, post) in &posts {
        store.put_post(*guild_id, post).await?;
    }
    println!("Imported {} daily posts", posts.len());

    Ok(())
}
