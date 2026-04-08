use crate::config::AppConfig;
use crate::proxy::ProxyPool;
use crate::tixcraft_api::TixcraftApiBot;
use anyhow::{Context, Result};
use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "ticket-bot2")]
#[command(about = "Rust API-mode watcher for ticket-bot")]
pub struct Cli {
    #[arg(long, default_value = "config.yaml")]
    config: PathBuf,

    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    ShowConfig {
        #[arg(long)]
        event: Option<String>,
    },
    ShowProxy {
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
    ApiWatchDryRun {
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        fetch: bool,
        #[arg(long)]
        resolve_targets: bool,
        #[arg(long, default_value_t = 5.0)]
        interval: f64,
    },
    Watch {
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 5.0)]
        interval: f64,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose)?;

    let config = AppConfig::load_from_path(&cli.config)?;
    let command = cli.command.unwrap_or(Command::ShowConfig { event: None });

    match command {
        Command::ShowConfig { event } => show_config(&config, event.as_deref()),
        Command::ShowProxy { session, count } => show_proxy(&config, session.as_deref(), count),
        Command::ApiWatchDryRun {
            event,
            session,
            fetch,
            resolve_targets,
            interval,
        } => {
            api_watch_dry_run(
                &config,
                event.as_deref(),
                session.as_deref(),
                fetch,
                resolve_targets,
                interval,
            )
            .await
        }
        Command::Watch {
            event,
            session,
            interval,
        } => watch(&config, event.as_deref(), session.as_deref(), interval).await,
    }
}

fn init_tracing(verbose: u8) -> Result<()> {
    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

fn show_config(config: &AppConfig, event_filter: Option<&str>) -> Result<()> {
    let event = config
        .select_event(event_filter)
        .context("no matching event found in config")?;

    println!("config loaded");
    println!("  events: {}", config.events.len());
    println!("  sessions: {}", config.sessions.len());
    println!(
        "  deployment: {}",
        empty_as_dash(&config.deployment.profile)
    );
    println!("  api_mode: {}", config.browser.api_mode);
    println!("  selected event: {}", event.name);
    println!("  event url: {}", event.url);
    println!("  date keyword: {}", empty_as_dash(&event.date_keyword));
    println!("  area keyword: {}", empty_as_dash(&event.area_keyword));
    println!("  proxy enabled: {}", config.proxy.enabled);
    println!("  proxy rotate: {}", config.proxy.rotate);

    Ok(())
}

fn show_proxy(config: &AppConfig, session_name: Option<&str>, count: usize) -> Result<()> {
    let session = config
        .select_session(session_name)
        .context("no matching session found in config")?;
    let pool = ProxyPool::new(config.proxy.clone());

    println!("session: {}", session.name);

    if !session.proxy_server.trim().is_empty() {
        println!("fixed proxy: {}", session.proxy_server);
        return Ok(());
    }

    if !pool.available() {
        println!("proxy pool disabled");
        return Ok(());
    }

    for idx in 0..count {
        let proxy = pool
            .next()
            .context("proxy pool unexpectedly returned None")?;
        println!("proxy[{idx}]: {proxy}");
    }

    Ok(())
}

async fn api_watch_dry_run(
    config: &AppConfig,
    event_filter: Option<&str>,
    session_name: Option<&str>,
    fetch: bool,
    resolve_targets: bool,
    interval: f64,
) -> Result<()> {
    let event = config
        .select_event(event_filter)
        .context("no matching event found in config")?;
    let session = config
        .select_session(session_name)
        .context("no matching session found in config")?;
    let bot = TixcraftApiBot::new(config, event, session)?;
    let plan = bot.plan();

    println!("api watch dry-run");
    println!("  event: {}", plan.event_name);
    println!("  url: {}", plan.event_url);
    println!("  session: {}", plan.session_name);
    println!("  profile: {}", plan.session_profile);
    println!("  cookie file: {}", empty_as_dash(&plan.cookie_file));
    println!("  api_mode: {}", plan.browser_api_mode);
    println!("  proxy: {}", plan.proxy.as_deref().unwrap_or("-"));
    println!("  user-agent: {}", plan.user_agent);
    println!("  request gap: {:.1}s", interval);

    if fetch {
        let result = bot.probe_event().await?;
        println!("probe result");
        println!("  status: {}", result.status);
        println!("  final url: {}", result.final_url);
        println!("  content-type: {}", empty_as_dash(&result.content_type));
        println!("  body bytes: {}", result.body_len);
    } else {
        println!("probe skipped");
        println!("  pass --fetch to make a real HTTP request");
    }

    if resolve_targets {
        let preview = bot.preview_watch_targets(interval).await?;
        println!("watch preview");
        println!("  target count: {}", preview.target_count);
        println!("  request gap: {:.1}s", preview.request_gap_secs);
        println!("  target refresh: {:.1}s", preview.target_refresh_secs);
        for (idx, target) in preview.targets.iter().enumerate() {
            println!("  target[{idx}]: {}", target.text);
            println!("    keyword: {}", target.keyword);
            println!("    href: {}", target.href);
        }
    } else {
        println!("watch preview skipped");
        println!("  pass --resolve-targets to fetch and list resolved watch targets");
    }

    Ok(())
}

async fn watch(
    config: &AppConfig,
    event_filter: Option<&str>,
    session_name: Option<&str>,
    interval: f64,
) -> Result<()> {
    let event = config
        .select_event(event_filter)
        .context("no matching event found in config")?;
    let session = config
        .select_session(session_name)
        .context("no matching session found in config")?;
    let mut bot = TixcraftApiBot::new(config, event, session)?;

    let result = bot.watch(interval).await;

    let stats = bot.stats();
    println!(
        "\nWatch 結束: {:.0}% ok ({}/{}), avg {:.0}ms",
        stats.success_rate() * 100.0,
        stats.ok,
        stats.total,
        stats.avg_latency_ms(),
    );
    if !bot.last_success_info().trim().is_empty() {
        println!("\n成功資訊\n{}", bot.last_success_info());
    }
    if !bot.last_error().trim().is_empty() {
        eprintln!("\n最後錯誤\n{}", bot.last_error());
    }

    result
}

fn empty_as_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}
