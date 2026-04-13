mod client;
mod config;
mod job;

use anyhow::Result;
use client::ServerClient;
use config::Config;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let client = ServerClient::new(config.clone())?;

    println!(
        "Polling {} for jobs as worker {}",
        config.job_url(),
        config.worker_id
    );
    println!(
        "Lifecycle callbacks configured: complete={}, failed={}",
        config.complete_path, config.failed_path
    );

    tokio::select! {
        result = run(client, config) => result,
        _ = signal::ctrl_c() => {
            println!("Shutdown requested");
            Ok(())
        }
    }
}

async fn run(client: ServerClient, config: Config) -> Result<()> {
    loop {
        match client.poll_next_job().await? {
            Some(job) => {
                let path = client.receive_job_file(&job).await?;
                println!("Received job {} -> {}", job.id, path.display());
            }
            None => {
                tokio::time::sleep(config.poll_interval).await;
            }
        }
    }
}
