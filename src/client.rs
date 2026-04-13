use crate::{
    config::Config,
    job::{Job, JobCompleteRequest, JobFailedRequest, JobResponse},
};
use anyhow::{Context, Result};
use reqwest::{Client, StatusCode, header};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs, io::AsyncWriteExt};
use url::Url;

pub struct ServerClient {
    http: Client,
    config: Config,
}

impl ServerClient {
    pub fn new(config: Config) -> Result<Self> {
        let mut headers = header::HeaderMap::new();

        if let Some(token) = &config.auth_token {
            let value = format!("Bearer {token}")
                .parse()
                .context("invalid auth token")?;
            headers.insert(header::AUTHORIZATION, value);
        }

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { http, config })
    }

    pub async fn poll_next_job(&self) -> Result<Option<Job>> {
        let response = self
            .http
            .get(self.config.job_url())
            .query(&[("worker_id", self.config.worker_id.as_str())])
            .send()
            .await
            .context("failed to poll job endpoint")?;

        if response.status() == StatusCode::NO_CONTENT || response.status() == StatusCode::NOT_FOUND
        {
            return Ok(None);
        }

        let response = response
            .error_for_status()
            .context("job endpoint returned an error")?;
        let job = response
            .json::<JobResponse>()
            .await
            .context("failed to decode job response")?
            .into_job();

        Ok(Some(job))
    }

    pub async fn receive_job_file(&self, job: &Job) -> Result<PathBuf> {
        let job_dir = self.config.work_dir.join(&job.id);
        fs::create_dir_all(&job_dir)
            .await
            .with_context(|| format!("failed to create work dir {}", job_dir.display()))?;

        let filename = self.resolve_filename(job)?;
        let final_path = job_dir.join(filename);
        let temp_path = final_path.with_extension("part");

        let mut response = self
            .http
            .get(Url::parse(&job.input_url).context("job input_url is not a valid URL")?)
            .send()
            .await
            .with_context(|| format!("failed to download job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("job {} download returned an error", job.id))?;

        let mut file = fs::File::create(&temp_path)
            .await
            .with_context(|| format!("failed to create {}", temp_path.display()))?;

        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed while streaming file")?
        {
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed to write {}", temp_path.display()))?;
        }

        file.flush()
            .await
            .with_context(|| format!("failed to flush {}", temp_path.display()))?;

        fs::rename(&temp_path, &final_path)
            .await
            .with_context(|| format!("failed to finalize {}", final_path.display()))?;

        Ok(final_path)
    }

    #[allow(dead_code)]
    pub async fn report_job_complete(&self, job: &Job, output_url: Option<&str>) -> Result<()> {
        let body = JobCompleteRequest {
            worker_id: &self.config.worker_id,
            output_url,
        };

        self.http
            .post(self.config.complete_url(&job.id))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to send complete callback for job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("complete callback rejected for job {}", job.id))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn report_job_failed(&self, job: &Job, error: &str) -> Result<()> {
        let body = JobFailedRequest {
            worker_id: &self.config.worker_id,
            error,
        };

        self.http
            .post(self.config.failed_url(&job.id))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to send failed callback for job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("failed callback rejected for job {}", job.id))?;

        Ok(())
    }

    fn resolve_filename(&self, job: &Job) -> Result<String> {
        if let Some(filename) = &job.filename {
            return Ok(sanitize_filename(filename));
        }

        let url = Url::parse(&job.input_url).context("job input_url is not a valid URL")?;
        let candidate = url
            .path_segments()
            .and_then(|segments| segments.last())
            .filter(|segment| !segment.is_empty())
            .unwrap_or("input.bin");

        Ok(sanitize_filename(candidate))
    }
}

fn sanitize_filename(name: &str) -> String {
    let trimmed = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("input.bin");

    let cleaned: String = trimmed
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect();

    if cleaned.is_empty() {
        "input.bin".to_string()
    } else {
        cleaned
    }
}
