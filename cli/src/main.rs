mod config;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use sqlx_migrator::cli::MigrationCommand;
use sqlx_migrator::migrator::{Migrate as _, Plan};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::config::{AppConfig, LogConfig};

#[derive(Parser)]
#[command(name = "imkitchen", version, about = "imkitchen-next CLI")]
struct Cli {
    #[arg(long, short = 'c', global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the web + admin HTTP servers.
    Serve,
    /// Database migrations (apply / revert / list / drop).
    Migrate(MigrationCommand),
    /// Revert every applied migration, then re-apply them all.
    Reset,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let cfg = config::load(cli.config.as_deref())?;
    init_tracing(&cfg.log)?;

    match cli.command {
        Command::Serve => serve(cfg).await,
        Command::Migrate(cmd) => migrate(cfg, cmd).await,
        Command::Reset => reset(cfg).await,
    }
}

fn init_tracing(cfg: &LogConfig) -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_new(&cfg.level)?;
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if cfg.json {
        builder.json().init();
    } else {
        builder.with_target(false).compact().init();
    }
    Ok(())
}

async fn migrate(cfg: AppConfig, cmd: MigrationCommand) -> Result<(), Box<dyn std::error::Error>> {
    let pool = imkitchen_db::create_pool(&cfg.database_url, 1).await?;
    let migrator = imkitchen_db::build_migrator()?;
    let mut conn = pool.acquire().await?;
    cmd.run(&mut *conn, Box::new(migrator)).await?;
    drop(conn);
    pool.close().await;
    Ok(())
}

async fn reset(cfg: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let pool = imkitchen_db::create_pool(&cfg.database_url, 1).await?;
    let migrator = imkitchen_db::build_migrator()?;
    let mut conn = pool.acquire().await?;
    migrator.run(&mut *conn, &Plan::revert_all()).await?;
    migrator.run(&mut *conn, &Plan::apply_all()).await?;
    drop(conn);
    pool.close().await;
    Ok(())
}

async fn serve(cfg: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let write_pool = imkitchen_db::create_write_pool(&cfg.database_url).await?;
    let read_pool = imkitchen_db::create_read_pool(
        &cfg.database_url,
        std::thread::available_parallelism()?.get() as u32,
    )
    .await?;

    // One evento executor backed by the write pool. The read pool is used
    // directly by projections for plain sqlx reads of `recipes_view` etc.
    let evento: evento::Sqlite = write_pool.clone().into();

    // Start the recipes context's projections + import saga. Returned handle
    // is shut down with the HTTP servers on signal. The same parser is also
    // injected into `AppState` so the web layer's multipart upload path uses
    // it for `parse_file`.
    let parser: std::sync::Arc<dyn imkitchen_recipes::import::RecipeParser> =
        std::sync::Arc::new(imkitchen_recipes::import::SeedParser);
    let recipes_subs = imkitchen_recipes::subscriptions::start_all(
        &evento,
        write_pool.clone(),
        parser.clone(),
    )
    .await?;

    let web_listener = tokio::net::TcpListener::bind(("0.0.0.0", cfg.web.port)).await?;
    let admin_listener = tokio::net::TcpListener::bind(("0.0.0.0", cfg.admin.port)).await?;

    tracing::info!(port = cfg.web.port, "web listening");
    tracing::info!(port = cfg.admin.port, "admin listening");

    let web_router = imkitchen_web_server::router(imkitchen_web_server::AppState {
        config: cfg.web,
        read_pool: read_pool.clone(),
        write_pool: write_pool.clone(),
        evento: evento.clone(),
        recipe_parser: parser,
    });
    let admin_router = imkitchen_admin_server::router(imkitchen_admin_server::AppState {
        config: cfg.admin,
        read_pool: read_pool.clone(),
        write_pool: write_pool.clone(),
    });

    let shutdown = CancellationToken::new();
    let shutdown_web = shutdown.clone();
    let shutdown_admin = shutdown.clone();

    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown_signal().await;
            shutdown.cancel();
        }
    });

    let web = axum::serve(web_listener, web_router)
        .with_graceful_shutdown(async move { shutdown_web.cancelled().await })
        .into_future();
    let admin = axum::serve(admin_listener, admin_router)
        .with_graceful_shutdown(async move { shutdown_admin.cancelled().await })
        .into_future();

    tokio::try_join!(web, admin)?;

    tracing::info!("shutting down recipes subscriptions");
    if let Err(err) = recipes_subs.shutdown().await {
        tracing::warn!(error = %err, "recipes subscriptions shutdown error");
    }

    tracing::info!("closing database pools");
    tokio::join!(write_pool.close(), read_pool.close());

    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Ctrl+C received, shutting down"),
        _ = terminate => tracing::info!("SIGTERM received, shutting down"),
    }
}
