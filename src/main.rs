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
    if config.debug_dry_run {
        println!("Debug dry run mode enabled");
    }

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
                if config.debug_dry_run {
                    let command = client.build_debug_ffmpeg_command(&job);
                    let transcode = job
                        .transcode
                        .as_ref()
                        .map(|spec| spec.summary())
                        .unwrap_or_else(|| "no transcode spec".to_string());
                    let delivery = job
                        .delivery
                        .as_ref()
                        .map(|spec| spec.summary())
                        .unwrap_or_else(|| "no delivery spec".to_string());

                    println!("Debug dry run for job {}:", job.id);
                    println!(
                        "  input: {}",
                        job.planned_input_path(&config.work_dir).display()
                    );
                    println!(
                        "  output: {}",
                        job.planned_output_path(&config.work_dir).display()
                    );
                    println!("  transcode: {}", transcode);
                    println!("  delivery: {}", delivery);
                    println!("  command: {command}");

                    let reason =
                        "debug dry run: printed planned ffmpeg command without downloading";
                    client.report_job_failed(&job, reason).await?;
                    println!("Marked job {} as failed for debug run", job.id);
                    return Ok(());
                } else {
                    let input_path = match client.receive_job_file(&job).await {
                        Ok(path) => path,
                        Err(err) => {
                            eprintln!("Failed to download job {}: {err:#}", job.id);
                            continue;
                        }
                    };

                    let transcode = job
                        .transcode
                        .as_ref()
                        .map(|spec| spec.summary())
                        .unwrap_or_else(|| "no transcode spec".to_string());
                    let delivery = job
                        .delivery
                        .as_ref()
                        .map(|spec| spec.summary())
                        .unwrap_or_else(|| "no delivery spec".to_string());

                    println!(
                        "Received job {} -> {} [{}; {}]",
                        job.id,
                        input_path.display(),
                        transcode,
                        delivery
                    );

                    match client.transcode_job_file(&job, &input_path).await {
                        Ok(output_path) => {
                            println!("Transcoded job {} -> {}", job.id, output_path.display());
                        }
                        Err(err) => {
                            eprintln!("Failed to transcode job {}: {err:#}", job.id);
                            continue;
                        }
                    }
                }
            }
            None => {
                tokio::time::sleep(config.poll_interval).await;
            }
        }
    }
}
