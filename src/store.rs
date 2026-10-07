//! DynamoDB persistence.
//!
//! | Table  | Key                          | Item                                         |
//! |--------|------------------------------|----------------------------------------------|
//! | guilds | `guild_id` (N)               | ServerConfig, plus the current `daily_post`  |
//! | users  | `guild_id` (N), `user_id` (N)| UserData                                     |
//!
//! Guild items have three writers: admin commands (config fields), the daily function's claim
//! (`last_cycle_at`), and its posting (`daily_post`). Each only updates its own attributes so
//! they can't overwrite each other.

use crate::data::{DailyPost, ServerConfig, UserData};
use aws_sdk_dynamodb::{types::AttributeValue, Client};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
type Item = HashMap<String, AttributeValue>;

fn number(id: u64) -> AttributeValue {
    AttributeValue::N(id.to_string())
}

#[derive(Clone)]
pub struct Store {
    client: Client,
    guilds_table: String,
    users_table: String,
}

impl Store {
    pub fn new(client: Client, guilds_table: String, users_table: String) -> Self {
        Self { client, guilds_table, users_table }
    }

    /// Build a store from the default AWS config and the GUILDS_TABLE / USERS_TABLE
    /// environment variables
    pub async fn from_env() -> Result<Self, Error> {
        let config = aws_config::load_from_env().await;
        let guilds_table = std::env::var("GUILDS_TABLE").map_err(|_| "GUILDS_TABLE environment variable is required")?;
        let users_table = std::env::var("USERS_TABLE").map_err(|_| "USERS_TABLE environment variable is required")?;
        Ok(Self::new(Client::new(&config), guilds_table, users_table))
    }

    // ---- Guilds ----

    pub async fn get_config(&self, guild_id: u64) -> Result<Option<ServerConfig>, Error> {
        let output = self.client
            .get_item()
            .table_name(&self.guilds_table)
            .key("guild_id", number(guild_id))
            .send()
            .await?;

        match output.item {
            Some(item) => Ok(Some(serde_dynamo::from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Create or update a guild's configuration. Leaves `last_cycle_at` and `daily_post` alone.
    pub async fn save_config(&self, config: &ServerConfig) -> Result<(), Error> {
        self.client
            .update_item()
            .table_name(&self.guilds_table)
            .key("guild_id", number(config.guild_id))
            .update_expression(
                "SET checkin_channel_id = :channel, timezone = :timezone, daily_time = :time, \
                 created_at = if_not_exists(created_at, :created_at), updated_at = :updated_at",
            )
            .expression_attribute_values(":channel", serde_dynamo::to_attribute_value(config.checkin_channel_id)?)
            .expression_attribute_values(":timezone", AttributeValue::S(config.timezone.clone()))
            .expression_attribute_values(":time", AttributeValue::S(config.daily_time.clone()))
            .expression_attribute_values(":created_at", serde_dynamo::to_attribute_value(config.created_at)?)
            .expression_attribute_values(":updated_at", serde_dynamo::to_attribute_value(config.updated_at)?)
            .send()
            .await?;
        Ok(())
    }

    pub async fn list_configs(&self) -> Result<Vec<ServerConfig>, Error> {
        let mut results = Vec::new();
        let mut start_key: Option<Item> = None;

        loop {
            let output = self.client
                .scan()
                .table_name(&self.guilds_table)
                .set_exclusive_start_key(start_key)
                .send()
                .await?;

            for item in output.items.unwrap_or_default() {
                results.push(serde_dynamo::from_item(item)?);
            }

            match output.last_evaluated_key {
                Some(key) => start_key = Some(key),
                None => break,
            }
        }

        Ok(results)
    }

    /// Atomically claim the daily cycle scheduled for `scheduled_at`.
    /// Returns false if this cycle (or a later one) was already claimed.
    pub async fn claim_cycle(&self, guild_id: u64, scheduled_at: DateTime<Utc>) -> Result<bool, Error> {
        let result = self.client
            .update_item()
            .table_name(&self.guilds_table)
            .key("guild_id", number(guild_id))
            .update_expression("SET last_cycle_at = :at")
            .condition_expression("attribute_exists(guild_id) AND (attribute_not_exists(last_cycle_at) OR attribute_type(last_cycle_at, :null) OR last_cycle_at < :at)")
            .expression_attribute_values(":at", serde_dynamo::to_attribute_value(scheduled_at)?)
            .expression_attribute_values(":null", AttributeValue::S("NULL".to_string()))
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(e) if e.as_service_error().is_some_and(|e| e.is_conditional_check_failed_exception()) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_post(&self, guild_id: u64) -> Result<Option<DailyPost>, Error> {
        let output = self.client
            .get_item()
            .table_name(&self.guilds_table)
            .key("guild_id", number(guild_id))
            .projection_expression("daily_post")
            .send()
            .await?;

        match output.item.and_then(|mut item| item.remove("daily_post")) {
            Some(AttributeValue::Null(_)) | None => Ok(None),
            Some(post) => Ok(Some(serde_dynamo::from_attribute_value(post)?)),
        }
    }

    /// Record a guild's current daily post. The guild must already exist.
    pub async fn put_post(&self, guild_id: u64, post: &DailyPost) -> Result<(), Error> {
        self.client
            .update_item()
            .table_name(&self.guilds_table)
            .key("guild_id", number(guild_id))
            .update_expression("SET daily_post = :post")
            .condition_expression("attribute_exists(guild_id)")
            .expression_attribute_values(":post", serde_dynamo::to_attribute_value(post)?)
            .send()
            .await?;
        Ok(())
    }

    // ---- Users ----

    pub async fn get_user(&self, guild_id: u64, user_id: u64) -> Result<Option<UserData>, Error> {
        let output = self.client
            .get_item()
            .table_name(&self.users_table)
            .key("guild_id", number(guild_id))
            .key("user_id", number(user_id))
            .send()
            .await?;

        match output.item {
            Some(item) => Ok(Some(serde_dynamo::from_item(item)?)),
            None => Ok(None),
        }
    }

    pub async fn put_user(&self, user: &UserData) -> Result<(), Error> {
        self.client
            .put_item()
            .table_name(&self.users_table)
            .set_item(Some(Self::to_item(user)?))
            .send()
            .await?;
        Ok(())
    }

    /// Write a user only if nobody else has modified it since it was read (optimistic locking
    /// on `updated_at`). Returns false if the write lost a race.
    pub async fn put_user_if_unchanged(&self, user: &UserData, read_updated_at: DateTime<Utc>) -> Result<bool, Error> {
        let result = self.client
            .put_item()
            .table_name(&self.users_table)
            .set_item(Some(Self::to_item(user)?))
            .condition_expression("updated_at = :read_updated_at")
            .expression_attribute_values(":read_updated_at", serde_dynamo::to_attribute_value(read_updated_at)?)
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(e) if e.as_service_error().is_some_and(|e| e.is_conditional_check_failed_exception()) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn list_users(&self, guild_id: u64) -> Result<Vec<UserData>, Error> {
        let mut results = Vec::new();
        let mut start_key: Option<Item> = None;

        loop {
            let output = self.client
                .query()
                .table_name(&self.users_table)
                .key_condition_expression("guild_id = :guild_id")
                .expression_attribute_values(":guild_id", number(guild_id))
                .set_exclusive_start_key(start_key)
                .send()
                .await?;

            for item in output.items.unwrap_or_default() {
                results.push(serde_dynamo::from_item(item)?);
            }

            match output.last_evaluated_key {
                Some(key) => start_key = Some(key),
                None => break,
            }
        }

        Ok(results)
    }

    fn to_item<T: Serialize>(value: &T) -> Result<Item, Error> {
        Ok(serde_dynamo::to_item(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stored_as_numbers() {
        // Key attributes must match the tables' numeric key schema
        let user: UserData = serde_json::from_str(r#"{"guild_id":779587086713225256,"user_id":586202950116966402,
            "goal":"g","current_streak":0,"longest_streak":0,"last_checkin_date":null,"grace_period_start":null,
            "is_active":true,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#).unwrap();
        let item = Store::to_item(&user).unwrap();
        assert_eq!(item["guild_id"], AttributeValue::N("779587086713225256".to_string()));
        assert_eq!(item["user_id"], AttributeValue::N("586202950116966402".to_string()));

        let back: UserData = serde_dynamo::from_item(item).unwrap();
        assert_eq!(back.user_id, 586202950116966402);
    }

    #[test]
    fn unset_last_cycle_at_reads_back_as_none() {
        // claim_cycle relies on configs without a claim reading as None
        let mut item: Item = HashMap::new();
        item.insert("guild_id".to_string(), number(42));
        item.insert("checkin_channel_id".to_string(), AttributeValue::Null(true));
        item.insert("timezone".to_string(), AttributeValue::S("UTC".to_string()));
        item.insert("daily_time".to_string(), AttributeValue::S("09:00".to_string()));
        item.insert("created_at".to_string(), AttributeValue::S("2026-01-01T00:00:00Z".to_string()));
        item.insert("updated_at".to_string(), AttributeValue::S("2026-01-01T00:00:00Z".to_string()));

        let config: ServerConfig = serde_dynamo::from_item(item).unwrap();
        assert_eq!(config.guild_id, 42);
        assert!(config.last_cycle_at.is_none());
    }

    #[test]
    fn timestamps_compare_chronologically_as_strings() {
        // claim_cycle compares timestamps with `<` in DynamoDB, which is lexicographic
        let earlier: DateTime<Utc> = "2026-10-05T09:00:00Z".parse().unwrap();
        let later: DateTime<Utc> = "2026-10-06T09:00:00Z".parse().unwrap();
        let as_string = |t| match serde_dynamo::to_attribute_value(t).unwrap() {
            AttributeValue::S(s) => s,
            other => panic!("expected string, got {:?}", other),
        };
        assert!(as_string(earlier) < as_string(later));
    }
}
