mod captcha;
mod cli;
mod config;
mod cookies;
mod http_client;
mod parser;
mod proxy;
mod tixcraft_api;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
