//! One-time import of the VPS bot's JSON data file into DynamoDB:
//!
//!     TABLE_NAME=... cargo run --bin import-data -- bot_data.json [--resume-on YYYY-MM-DD] [--dry-run]
//!
//! --resume-on  Local date (in each server's timezone) of the first daily post the Lambda will
//!              make. Active streaks are carried across the gap since the old bot's last post.
//! --dry-run    Print what would be imported without writing to DynamoDB.

use chrono::NaiveDate;
use daily_checkin_bot::{
    daily,
    data::{DailyPost, ServerConfig, UserData},
    store::{Error, Store},
    streaks::StreakManager,
};
use serde::Deserialize;
use std::collections::HashMap;

/// Shape of the old bot_data.json file
#[derive(Deserialize)]
struct LegacyBotData {
    servers: HashMap<String, ServerConfig>,
    users: HashMap<String, HashMap<String, UserData>>, // guild_id -> user_id -> UserData
    daily_posts: HashMap<String, DailyPost>, // guild_id -> current post
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

    let mut data: LegacyBotData = serde_json::from_str(&std::fs::read_to_string(&path)?)?;

    if let Some(local_date) = resume_on {
        for (guild_id, users) in data.users.iter_mut() {
            let config = data.servers.get(guild_id).ok_or_else(|| format!("No server config for guild {}", guild_id))?;
            let first_post = daily::scheduled_at(config, local_date)?
                .ok_or_else(|| format!("{} {} doesn't exist in {}", local_date, config.daily_time, config.timezone))?;
            println!("Guild {}: first post at {}", guild_id, first_post);

            for user in users.values_mut() {
                let last_checkin = user.last_checkin_date;
                if StreakManager::bridge_outage(user, first_post.date_naive()) {
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

    let user_count: usize = data.users.values().map(|users| users.len()).sum();
    if dry_run {
        println!("Dry run: would import {} server configs, {} users, {} daily posts",
            data.servers.len(), user_count, data.daily_posts.len());
        return Ok(());
    }

    let store = Store::from_env().await?;

    for config in data.servers.values() {
        store.put_config(config).await?;
    }
    println!("Imported {} server configs", data.servers.len());

    for (guild_id, users) in &data.users {
        for user in users.values() {
            store.put_user(guild_id, user).await?;
        }
    }
    println!("Imported {} users", user_count);

    for post in data.daily_posts.values() {
        store.put_post(post).await?;
    }
    println!("Imported {} daily posts", data.daily_posts.len());

    Ok(())
}
