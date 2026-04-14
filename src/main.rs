mod client;
mod config;
mod job;

use anyhow::Result;
use client::ServerClient;
use config::Config;
use gstreamer as gst;
use job::Job;
fn main() {
    // 1. GLOBAL INITIALIZATION (Do this once)
    gst::init().expect("Failed to initialize GStreamer");

    // 2. REGISTER STATIC PLUGINS (Must be after init)
    gstrav1e::plugin_register_static().expect("Failed to bundle rav1e plugin");
    let config = Config::from_env().unwrap();
    let client = ServerClient::new(config.clone()).unwrap();

    run(client, config).unwrap();
}

fn run(client: ServerClient, config: Config) -> Result<()> {
    loop {
        match client.poll_next_job()? {
            Some(job) => {
                process_job(&client, &job)?;
            }
            None => {
                std::thread::sleep(config.poll_interval);
            }
        }
    }
}

fn process_job(client: &ServerClient, job: &Job) -> Result<()> {
    let input_path = match client.receive_job_file(job) {
        Ok(path) => path,
        Err(err) => {
            eprintln!(
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

    println!(
        "Received job {} from {} -> {} [{}; {}]",
        job.id,
        job.input_url,
        job.output_url,
        input_path.display(),
        transcode,
    );

    let output_path = match client.transcode_job_file(job, &input_path) {
        Ok(path) => path,
        Err(err) => {
            eprintln!(
                "Failed to transcode job {} from {}: {err:#}",
                job.id, job.input_url
            );
            fail_and_cleanup(client, job, "transcode failed");
            return Ok(());
        }
    };
    println!("Transcoded job {} -> {}", job.id, output_path.display());

    if let Err(err) = client.upload_job_output(job, &output_path) {
        eprintln!("Failed to upload job {} files: {err:#}", job.id);
        fail_and_cleanup(client, job, &err.to_string());
        return Ok(());
    } else {
        println!("Cleaned uploaded files for job {}", job.id);
    }
    if let Err(err) = client.report_job_complete(job) {
        eprintln!(
            "Failed to report completion for job {} from {}: {err:#}",
            job.id, job.input_url
        );
        if let Err(cleanup_err) = client.cleanup_job_files(job) {
            eprintln!("Failed to clean up job {} files: {cleanup_err:#}", job.id);
        } else {
            println!("Cleaned up local files for job {}", job.id);
        }
        return Ok(());
    }
    if let Err(err) = client.cleanup_job_files(job) {
        eprintln!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        println!("Cleaned up local files for job {}", job.id);
    }

    Ok(())
}

fn fail_and_cleanup(client: &ServerClient, job: &Job, reason: &str) {
    if let Err(err) = client.report_job_failed(job, reason) {
        eprintln!(
            "Failed to report failure for job {} from {} after {reason}: {err:#}",
            job.id, job.input_url
        );
    }

    if let Err(err) = client.cleanup_job_files(job) {
        eprintln!("Failed to clean up job {} files: {err:#}", job.id);
    } else {
        println!("Cleaned up local files for job {}", job.id);
    }
}
