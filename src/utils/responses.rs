use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

pub fn success_response(message: &str) -> CreateInteractionResponse {
    let data = CreateInteractionResponseMessage::new().content(format!("✅ {}", message));
    CreateInteractionResponse::Message(data)
}

pub fn error_response(message: &str) -> CreateInteractionResponse {
    let data = CreateInteractionResponseMessage::new().content(format!("❌ {}", message));
    CreateInteractionResponse::Message(data)
}

pub fn info_response(message: &str) -> CreateInteractionResponse {
    let data = CreateInteractionResponseMessage::new().content(format!("ℹ️ {}", message));
    CreateInteractionResponse::Message(data)
}