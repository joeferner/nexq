//! `nexq-server` binary. The only crate that knows about all the others: it loads
//! config, then runs whichever facades are enabled.
//!
//! Each facade owns its own listener, so this is a supervisor rather than a host — it
//! binds nothing itself. A facade that is disabled in config is never even bound.
//!
//! Every backend and facade is linked in, and config alone decides what runs.

use std::error::Error;
use std::process::ExitCode;
use std::sync::Arc;

use nexq_core::Config;
use tokio::signal;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

type BoxError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), BoxError> {
    let config = Config::load()?;

    // One credential registry, shared by every facade — each presents the same
    // principals in whatever way its protocol expects.
    let auth = Arc::new(config.auth.clone());

    // Broadcasts "stop" to every facade at once, so one signal drains them all.
    let (shutdown_tx, _) = watch::channel(false);
    let mut facades = JoinSet::new();

    if config.aws_api.enabled {
        // Bind before spawning, so a port conflict fails startup rather than
        // surfacing later from inside a task.
        let server = nexq_api_aws::Server::bind(&config.aws_api, Arc::clone(&auth)).await?;
        facades.spawn(server.serve(shutdown_on(shutdown_tx.subscribe())));
    }

    if facades.is_empty() {
        return Err("no facade is enabled, so there is nothing to serve".into());
    }

    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        info!("signal received, shutting down");
        let _ = signal_tx.send(true);
    });

    let mut failure: Option<BoxError> = None;
    while let Some(joined) = facades.join_next().await {
        let outcome = match joined {
            Ok(served) => served.map_err(BoxError::from),
            Err(join_error) => Err(BoxError::from(join_error)),
        };

        if let Err(error) = outcome {
            // A half-serving deployment is worse than a stopped one: take the rest
            // down too, and report the first failure.
            let _ = shutdown_tx.send(true);
            failure.get_or_insert(error);
        }
    }

    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Resolves when shutdown is signalled.
async fn shutdown_on(mut shutdown: watch::Receiver<bool>) {
    // An error means the sender is gone, which is also a reason to stop.
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// Resolves on the first signal asking the process to stop.
async fn wait_for_signal() {
    #[cfg(unix)]
    {
        // SIGTERM is what Kubernetes and `docker stop` send; Ctrl-C is for the terminal.
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler should install");

        tokio::select! {
            _ = signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    let _ = signal::ctrl_c().await;
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}
