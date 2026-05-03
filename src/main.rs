mod client;
mod config;
mod job;

use anyhow::{Context, Result};
use client::ServerClient;
use config::Config;
use job::Job;
use log::{error, info, warn};
use std::{sync::Arc, thread, time::Duration};
fn main() {
    pretty_env_logger::init_timed();
    let config = Config::from_env().unwrap();
    const NUMBER_OF_TIMES_TO_RUN: usize = 5;
    let client_sems = Arc::new(client::ClientSems::new(&config));
    for i in 0..NUMBER_OF_TIMES_TO_RUN {
        let client = ServerClient::new(config.clone(), client_sems.clone(), i).unwrap();
        thread::spawn(move || run(client).unwrap());
    }
    thread::sleep(Duration::MAX);
}

fn run(client: ServerClient) -> Result<()> {
    let mut error_sleep_time = client.poll_interval();
    let max_sleep_time = client.poll_interval() * 64;
    loop {
        match client.poll_next_job() {
            Ok(Some(job)) => {
                if let Err(err) = process_job(&client, &job) {
                    if let Err(err_inner) = cleanup_and_fail(&client, &job, &err.to_string()) {
                        error!("error: {}", err_inner);
                    };
                };
            }
            Ok(None) => {
                error_sleep_time = client.poll_interval();
                std::thread::sleep(client.poll_interval());
            }
            Err(e) => {
                warn!("{}", e);
                if error_sleep_time < max_sleep_time {
                    error_sleep_time *= 2;
                } else {
                    error_sleep_time = max_sleep_time;
                }
                std::thread::sleep(error_sleep_time);
            }
        }
    }
}

fn process_job(client: &ServerClient, job: &Job) -> Result<()> {
    info!(
        "{}: Received job {} from {} -> {}",
        client, job.id, job.input_url, job.output_url,
    );
    let input_path = client
        .receive_job_file(job)
        .with_context(|| format!("Failed to download job {} from {}", job.id, job.input_url,))?;
    let transcode = job
        .transcode
        .as_ref()
        .map(|spec| spec.summary())
        .unwrap_or_else(|| "no transcode spec".to_string());
    info!(
        "{}: Transcoding {} with args {}",
        client,
        input_path.display(),
        transcode
    );
    let output_path = client
        .transcode_job_file(job, &input_path)
        .with_context(|| format!("Failed to transcode job {} from {} ", job.id, job.input_url))?;
    info!(
        "{}: Transcoded job {} -> {}",
        client,
        job.id,
        output_path.display()
    );
    client.upload_job_output(job, &output_path)?;
    client.report_job_complete(job)?;
    Ok(())
}
fn cleanup_and_fail(client: &ServerClient, job: &Job, error: &str) -> Result<()> {
    client.report_job_failed(job, error)?;
    client.cleanup_job_files()?;
    Ok(())
}
