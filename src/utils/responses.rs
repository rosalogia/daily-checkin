use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage, CreateEmbed};

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

pub fn embed_response(embed: CreateEmbed) -> CreateInteractionResponse {
    let data = CreateInteractionResponseMessage::new().add_embed(embed);
    CreateInteractionResponse::Message(data)
}