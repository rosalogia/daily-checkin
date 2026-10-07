//! Lambda behind a Function URL that serves as the Discord Interactions Endpoint:
//! slash commands and button clicks.

use daily_checkin_bot::{commands::{self, App}, store::Store};
use lambda_http::{run, service_fn, Body, Error, Request, Response};
use serenity::{interactions_endpoint::Verifier, model::application::Interaction};
use tracing::warn;

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_http::tracing::init_default_subscriber();

    let public_key = std::env::var("DISCORD_PUBLIC_KEY")
        .map_err(|_| "DISCORD_PUBLIC_KEY environment variable is required")?;
    let verifier = Verifier::new(&public_key);

    let aws_config = aws_config::load_from_env().await;
    let app = App {
        store: Store::from_env().await?,
        lambda: aws_sdk_lambda::Client::new(&aws_config),
        daily_function: std::env::var("DAILY_FUNCTION_NAME")
            .map_err(|_| "DAILY_FUNCTION_NAME environment variable is required")?,
    };

    run(service_fn(|request| handle(&app, &verifier, request))).await
}

async fn handle(app: &App, verifier: &Verifier, request: Request) -> Result<Response<Body>, Error> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok());
    let body: &[u8] = request.body().as_ref();

    // Discord requires rejecting requests with invalid signatures
    let verified = match (header("x-signature-ed25519"), header("x-signature-timestamp")) {
        (Some(signature), Some(timestamp)) => verifier.verify(signature, timestamp, body).is_ok(),
        _ => false,
    };
    if !verified {
        warn!("Rejected request with invalid signature");
        return Ok(Response::builder().status(401).body(Body::from("invalid request signature"))?);
    }

    let interaction: Interaction = serde_json::from_slice(body)?;
    let response = commands::handle_interaction(app, &interaction).await;

    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&response)?))?)
}
