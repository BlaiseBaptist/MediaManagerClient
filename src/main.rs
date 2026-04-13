mod client;
mod config;
mod job;

use anyhow::Result;
use client::ServerClient;
use config::Config;
use job::Job;
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
                    if let Err(err) = client.report_job_failed(&job, reason).await {
                        eprintln!(
                            "Failed to report debug dry-run failure for job {}: {err:#}",
                            job.id
                        );
                    }
                    if let Err(err) = client.cleanup_job_files(&job).await {
                        eprintln!(
                            "Failed to clean up debug dry-run files for job {}: {err:#}",
                            job.id
                        );
                    }
                    println!("Marked job {} as failed for debug run", job.id);
                    return Ok(());
                } else {
                    process_job(&client, &job).await?;
                }
            }
            None => {
                tokio::time::sleep(config.poll_interval).await;
            }
        }
    }
}

async fn process_job(client: &ServerClient, job: &Job) -> Result<()> {
    let input_path = match client.receive_job_file(job).await {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Failed to download job {}: {err:#}", job.id);
            fail_and_cleanup(client, job, "download failed").await;
            return Ok(());
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

    let output_path = match client.transcode_job_file(job, &input_path).await {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Failed to transcode job {}: {err:#}", job.id);
            fail_and_cleanup(client, job, "transcode failed").await;
            return Ok(());
        }
    };

    println!("Transcoded job {} -> {}", job.id, output_path.display());

    if let Some(output_url) = job
        .delivery
        .as_ref()
        .and_then(|delivery| delivery.output_url.as_deref())
    {
        if let Err(err) = client.upload_job_output(job, &output_path).await {
            eprintln!("Failed to upload job {} output: {err:#}", job.id);
            fail_and_cleanup(client, job, "upload failed").await;
            return Ok(());
        }

        println!("Uploaded job {} output to {}", job.id, output_url);
    } else {
        println!(
            "Job {} has no delivery.output_url; keeping output locally",
            job.id
        );
    }

    let output_url = job
        .delivery
        .as_ref()
        .and_then(|delivery| delivery.output_url.as_deref());
    if let Err(err) = client.report_job_complete(job, output_url).await {
        eprintln!("Failed to report completion for job {}: {err:#}", job.id);
        if let Err(cleanup_err) = client.cleanup_job_files(job).await {
            eprintln!("Failed to clean up job {} files: {cleanup_err:#}", job.id);
        } else {
            println!("Cleaned up local files for job {}", job.id);
        }
        return Ok(());
    }

    if let Err(err) = client.cleanup_job_files(job).await {
        eprintln!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        println!("Cleaned up local files for job {}", job.id);
    }

    Ok(())
}

async fn fail_and_cleanup(client: &ServerClient, job: &Job, reason: &str) {
    if let Err(err) = client.report_job_failed(job, reason).await {
        eprintln!(
            "Failed to report failure for job {} after {reason}: {err:#}",
            job.id
        );
    }

    if let Err(err) = client.cleanup_job_files(job).await {
        eprintln!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        println!("Cleaned up local files for job {}", job.id);
    }
}
