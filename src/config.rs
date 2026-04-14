use anyhow::{Context, Result};
use std::{env, path::PathBuf, time::Duration};
use url::Url;

#[derive(Clone, Debug)]
pub struct Config {
    pub server_base_url: Url,
    pub job_path: String,
    pub complete_path: String,
    pub failed_path: String,
    pub poll_interval: Duration,
    pub work_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let server_base_url = env::var("MEDIA_MANAGER_SERVER_URL")
            .context("MEDIA_MANAGER_SERVER_URL is required")?
            .parse::<Url>()
            .context("MEDIA_MANAGER_SERVER_URL must be a valid URL")?;

        let job_path = env::var("MEDIA_MANAGER_JOB_PATH")
            .unwrap_or_else(|_| "/api/worker/jobs/next".to_string());

        let complete_path = env::var("MEDIA_MANAGER_COMPLETE_PATH")
            .unwrap_or_else(|_| "/api/worker/jobs/{job_id}/complete".to_string());

        let failed_path = env::var("MEDIA_MANAGER_FAILED_PATH")
            .unwrap_or_else(|_| "/api/worker/jobs/{job_id}/failed".to_string());

        let poll_interval = env::var("MEDIA_MANAGER_POLL_INTERVAL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(5));

        let work_dir = env::var("MEDIA_MANAGER_WORK_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./.work"));

        Ok(Self {
            server_base_url,
            job_path,
            complete_path,
            failed_path,
            poll_interval,
            work_dir,
        })
    }

    pub fn job_url(&self) -> Url {
        self.server_base_url
            .join(self.job_path.trim_start_matches('/'))
            .expect("job path should be valid")
    }

    pub fn complete_url(&self, job_id: &str) -> Url {
        self.action_url(&self.complete_path, job_id)
    }

    pub fn failed_url(&self, job_id: &str) -> Url {
        self.action_url(&self.failed_path, job_id)
    }

    fn action_url(&self, template: &str, job_id: &str) -> Url {
        let path = template.replace("{job_id}", job_id);
        self.server_base_url
            .join(path.trim_start_matches('/'))
            .expect("action path should be valid")
    }
}
