//! DynamoDB persistence using a single table.
//!
//! | pk              | sk              | item         |
//! |-----------------|-----------------|--------------|
//! | `CONFIG`        | `GUILD#<guild>` | ServerConfig |
//! | `GUILD#<guild>` | `USER#<user>`   | UserData     |
//! | `GUILD#<guild>` | `POST`          | DailyPost    |
//!
//! Server configs share one partition so the daily job can load them all with a single query.

use crate::data::{DailyPost, ServerConfig, UserData};
use aws_sdk_dynamodb::{types::AttributeValue, Client};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
type Item = HashMap<String, AttributeValue>;

const CONFIG_PK: &str = "CONFIG";
const POST_SK: &str = "POST";

fn guild_pk(guild_id: &str) -> String {
    format!("GUILD#{}", guild_id)
}

fn config_sk(guild_id: &str) -> String {
    format!("GUILD#{}", guild_id)
}

fn user_sk(user_id: &str) -> String {
    format!("USER#{}", user_id)
}

#[derive(Clone)]
pub struct Store {
    client: Client,
    table: String,
}

impl Store {
    pub fn new(client: Client, table: String) -> Self {
        Self { client, table }
    }

    /// Build a store from the default AWS config and the TABLE_NAME environment variable
    pub async fn from_env() -> Result<Self, Error> {
        let config = aws_config::load_from_env().await;
        let table = std::env::var("TABLE_NAME").map_err(|_| "TABLE_NAME environment variable is required")?;
        Ok(Self::new(Client::new(&config), table))
    }

    // ---- Server configs ----

    pub async fn get_config(&self, guild_id: &str) -> Result<Option<ServerConfig>, Error> {
        self.get(CONFIG_PK, &config_sk(guild_id)).await
    }

    pub async fn put_config(&self, config: &ServerConfig) -> Result<(), Error> {
        self.put(CONFIG_PK, &config_sk(&config.guild_id), config).await
    }

    pub async fn list_configs(&self) -> Result<Vec<ServerConfig>, Error> {
        self.query(CONFIG_PK, "GUILD#").await
    }

    /// Atomically claim the daily cycle scheduled for `scheduled_at`.
    /// Returns false if this cycle (or a later one) was already claimed.
    pub async fn claim_cycle(&self, guild_id: &str, scheduled_at: DateTime<Utc>) -> Result<bool, Error> {
        let result = self.client
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(CONFIG_PK.to_string()))
            .key("sk", AttributeValue::S(config_sk(guild_id)))
            .update_expression("SET last_cycle_at = :at")
            .condition_expression("attribute_exists(pk) AND (attribute_not_exists(last_cycle_at) OR attribute_type(last_cycle_at, :null) OR last_cycle_at < :at)")
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

    // ---- Users ----

    pub async fn get_user(&self, guild_id: &str, user_id: &str) -> Result<Option<UserData>, Error> {
        self.get(&guild_pk(guild_id), &user_sk(user_id)).await
    }

    pub async fn put_user(&self, guild_id: &str, user: &UserData) -> Result<(), Error> {
        self.put(&guild_pk(guild_id), &user_sk(&user.user_id), user).await
    }

    /// Write a user only if nobody else has modified it since it was read (optimistic locking
    /// on `updated_at`). Returns false if the write lost a race.
    pub async fn put_user_if_unchanged(
        &self,
        guild_id: &str,
        user: &UserData,
        read_updated_at: DateTime<Utc>,
    ) -> Result<bool, Error> {
        let result = self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(Self::to_item(&guild_pk(guild_id), &user_sk(&user.user_id), user)?))
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

    pub async fn list_users(&self, guild_id: &str) -> Result<Vec<UserData>, Error> {
        self.query(&guild_pk(guild_id), "USER#").await
    }

    // ---- Daily posts ----

    pub async fn get_post(&self, guild_id: &str) -> Result<Option<DailyPost>, Error> {
        self.get(&guild_pk(guild_id), POST_SK).await
    }

    pub async fn put_post(&self, post: &DailyPost) -> Result<(), Error> {
        self.put(&guild_pk(&post.guild_id), POST_SK, post).await
    }

    // ---- Generic helpers ----

    fn to_item<T: Serialize>(pk: &str, sk: &str, value: &T) -> Result<Item, Error> {
        let mut item: Item = serde_dynamo::to_item(value)?;
        item.insert("pk".to_string(), AttributeValue::S(pk.to_string()));
        item.insert("sk".to_string(), AttributeValue::S(sk.to_string()));
        Ok(item)
    }

    async fn get<T: DeserializeOwned>(&self, pk: &str, sk: &str) -> Result<Option<T>, Error> {
        let output = self.client
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(pk.to_string()))
            .key("sk", AttributeValue::S(sk.to_string()))
            .send()
            .await?;

        match output.item {
            Some(item) => Ok(Some(serde_dynamo::from_item(item)?)),
            None => Ok(None),
        }
    }

    async fn put<T: Serialize>(&self, pk: &str, sk: &str, value: &T) -> Result<(), Error> {
        self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(Self::to_item(pk, sk, value)?))
            .send()
            .await?;
        Ok(())
    }

    async fn query<T: DeserializeOwned>(&self, pk: &str, sk_prefix: &str) -> Result<Vec<T>, Error> {
        let mut results = Vec::new();
        let mut start_key: Option<Item> = None;

        loop {
            let output = self.client
                .query()
                .table_name(&self.table)
                .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.to_string()))
                .expression_attribute_values(":prefix", AttributeValue::S(sk_prefix.to_string()))
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_carry_keys_and_round_trip() {
        let config = ServerConfig::new("42".to_string());
        let item = Store::to_item(CONFIG_PK, &config_sk("42"), &config).unwrap();

        assert_eq!(item["pk"], AttributeValue::S("CONFIG".to_string()));
        assert_eq!(item["sk"], AttributeValue::S("GUILD#42".to_string()));
        // claim_cycle relies on an unset last_cycle_at being stored as NULL
        assert_eq!(item["last_cycle_at"], AttributeValue::Null(true));

        let back: ServerConfig = serde_dynamo::from_item(item).unwrap();
        assert_eq!(back.guild_id, "42");
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
