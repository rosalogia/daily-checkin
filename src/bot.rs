use crate::data::BotData;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SharedBotData = Arc<RwLock<BotData>>;

pub struct Bot {
    pub data: SharedBotData,
}

impl Bot {
    pub fn new(data: BotData) -> Self {
        Self {
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub async fn save_data(&self) -> anyhow::Result<()> {
        let data = self.data.read().await;
        data.save().await
    }
}