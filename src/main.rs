mod client;
mod config;
mod job;

use anyhow::Result;
use client::ServerClient;
use config::Config;
use job::Job;
use log::{error, info};
use std::{sync::Arc, thread, time::Duration};
fn main() {
    pretty_env_logger::init_timed();
    let config = Config::from_env().unwrap();
    const NUMBER_OF_TIMES_TO_RUN: usize = 5;
    let client_sems = Arc::new(client::ClientSems::new());
    for i in 0..NUMBER_OF_TIMES_TO_RUN {
        let client = ServerClient::new(config.clone(), client_sems.clone(), i).unwrap();
        thread::spawn(move || run(client).unwrap());
    }
    thread::sleep(Duration::MAX);
}

fn run(client: ServerClient) -> Result<()> {
    loop {
        match client.poll_next_job() {
            Ok(Some(job)) => {
                process_job(&client, &job)?;
            }
            Ok(None) => {
                std::thread::sleep(client.poll_interval());
            }
            Err(e) => {
                info!("{}", e);
                //make backoff
                std::thread::sleep(client.poll_interval() * 2);
            }
        }
    }
}

fn process_job(client: &ServerClient, job: &Job) -> Result<()> {
    info!(
        "Received job {} from {} -> {}",
        job.id, job.input_url, job.output_url,
    );
    let input_path = match client.receive_job_file(job) {
        Ok(path) => path,
        Err(err) => {
            error!(
                "Failed to download job {} from {}: {err:#}",
                job.id, job.input_url
            );
            fail_and_cleanup(client, job, "download failed");
            return Ok(());
        }
    };
    let transcode = job
        .transcode
        .as_ref()
        .map(|spec| spec.summary())
        .unwrap_or_else(|| "no transcode spec".to_string());
    info!(
        "Transcoding {} with args {}",
        input_path.display(),
        transcode
    );
    let output_path = match client.transcode_job_file(job, &input_path) {
        Ok(path) => path,
        Err(err) => {
            error!(
                "Failed to transcode job {} from {}: {err:#}",
                job.id, job.input_url
            );
            fail_and_cleanup(client, job, "transcode failed");
            return Ok(());
        }
    };
    info!("Transcoded job {} -> {}", job.id, output_path.display());
    if let Err(err) = client.upload_job_output(job, &output_path) {
        error!("Failed to upload job {} files: {err:#}", job.id);
        fail_and_cleanup(client, job, &err.to_string());
        return Ok(());
    } else {
        info!("Uploaded files for job {}", job.id);
    }
    if let Err(err) = client.report_job_complete(job) {
        error!(
            "Failed to report completion for job {} from {}: {err:#}",
            job.id, job.input_url
        );
        if let Err(cleanup_err) = client.cleanup_job_files() {
            error!("Failed to clean up job {} files: {cleanup_err:#}", job.id);
        } else {
            info!("Cleaned up local files for job {}", job.id);
        }
        return Ok(());
    }
    if let Err(err) = client.cleanup_job_files() {
        error!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        info!("Cleaned up local files for job {}", job.id);
    }

    Ok(())
}

fn fail_and_cleanup(client: &ServerClient, job: &Job, reason: &str) {
    if let Err(err) = client.report_job_failed(job, reason) {
        error!(
            "Failed to report failure for job {} from {} after {reason}: {err:#}",
            job.id, job.input_url
        );
    }

    if let Err(err) = client.cleanup_job_files() {
        error!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        info!("Cleaned up local files for job {}", job.id);
    }
}
