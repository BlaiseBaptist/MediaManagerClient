use crate::{
    config::Config,
    job::{Job, JobCompleteRequest, JobFailedRequest, JobResponse},
};
use anyhow::{Context, Result};
use log::{debug, warn};
use reqwest::{StatusCode, blocking::Client};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use std_semaphore::Semaphore;
use tokio::{fs, io::AsyncWriteExt};
use url::Url;
pub struct ClientSems {
    download: Semaphore,
    upload: Semaphore,
    transcode: Semaphore,
}
impl ClientSems {
    pub fn new(config: &Config) -> Self {
        Self {
            download: Semaphore::new(config.downloads),
            upload: Semaphore::new(config.uploads),
            transcode: Semaphore::new(config.transcodes),
        }
    }
}
pub struct ServerClient {
    http: Client,
    config: Config,
    client_sems: Arc<ClientSems>,
    id: usize,
}
impl std::fmt::Display for ServerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "Client {}", self.id)
    }
}
impl ServerClient {
    pub fn new(config: Config, client_sems: Arc<ClientSems>, id: usize) -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::new(),
            config,
            client_sems,
            id,
        })
    }
    pub fn poll_interval(&self) -> Duration {
        return self.config.poll_interval;
    }
    pub fn poll_next_job(&self) -> Result<Option<Job>> {
        let response = self.http.get(self.config.job_url()).send()?;

        if response.status() == StatusCode::NO_CONTENT || response.status() == StatusCode::NOT_FOUND
        {
            return Ok(None);
        }

        let job = response
            .json::<JobResponse>()
            .context("failed to decode job response")?
            .into_job();

        Ok(Some(job))
    }

    pub fn receive_job_file(&self, job: &Job) -> Result<PathBuf> {
        let _guard = self.client_sems.download.access();
        let final_path = self.config.work_dir.join(format!("in{}.mkv", self.id));
        let temp_path = final_path.with_extension("part");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let download_url = Url::parse(&format!(
                "http://{}{}",
                self.config
                    .server_base_url
                    .host()
                    .context("invaild MEDIA_MANAGER_SERVER_URL")?,
                &job.input_url
            ))
            .context("job input_url is not a valid URL")?;
            debug!("{}: Downloading from: {}", self, download_url);
            let mut response = reqwest::get(download_url)
                .await?
                .error_for_status()
                .with_context(|| format!("job {} download returned an error", job.id))?;
            let mut file: tokio::fs::File = fs::File::create(&temp_path)
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
            Ok::<(), anyhow::Error>(())
        })?;
        std::fs::rename(&temp_path, &final_path)
            .with_context(|| format!("failed to finalize {}", final_path.display()))?;

        Ok(final_path)
    }

    fn get_best_av1_encoder() -> String {
        let candidates = ["av1_nvenc", "av1_qsv", "av1_vaapi", "libsvtav1", "librav1e"];

        for name in candidates {
            if Self::is_encoder_functional(name) {
                return name.to_string();
            }
        }

        "librav1e".to_string()
    }
    fn is_encoder_functional(encoder: &str) -> bool {
        let status = Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "nullsrc=s=256x256:r=1",
                "-frames:v",
                "1",
                "-c:v",
                encoder,
                "-f",
                "null",
                "-",
            ])
            .output();
        match status {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    pub fn transcode_job_file(&self, job: &Job, input_path: &Path) -> Result<PathBuf> {
        let _guard = self.client_sems.transcode.access();
        let output_path = input_path.with_file_name(format!("out{}.mkv", self.id));
        let v_encoder = match job
            .transcode
            .as_ref()
            .and_then(|s| s.video_codec.as_deref())
        {
            Some("av1") | None => &Self::get_best_av1_encoder(),
            Some(other) => {
                warn!("using given encoder as string directly");
                other
            }
        };

        let mut cmd = Command::new("ffmpeg");

        cmd.arg("-y")
            .arg("-i")
            .arg(input_path)
            .arg("-c:v")
            .arg(v_encoder);
        match v_encoder {
            "libsvtav1" => cmd.args([
                "-preset",
                "4",
                "-crf",
                "20",
                "-svtav1-params",
                "tune=0:enable-overlays=1",
                "-pix_fmt",
                "yuv420p10le",
            ]),
            "librav1e" => cmd.args([
                "-speed",
                "3",
                "-qp",
                "60",
                "-tiles",
                "9",
                "-pix_fmt",
                "yuv420p10le",
            ]),
            "av1_nvenc" => cmd.args([
                "-preset", "p7", "-tune", "hq", "-rc", "constqp", "-qp", "18", "-b:v", "0",
                "-pix_fmt", "p010le",
            ]),
            "av1_vaapi" => cmd.args([
                "-rc_mode",
                "CQP",
                "-qp",
                "18",
                "-compression_level:v",
                "1",
                "-pix_fmt",
                "yuv420p10le",
            ]),
            "av1_qsv" => cmd.args([
                "-preset",
                "veryslow",
                "-global_quality:v",
                "20",
                "-look_ahead:v",
                "1",
                "-pix_fmt",
                "yuv420p10le",
            ]),
            _ => {
                todo!()
            }
        };

        let a_encoder = match job
            .transcode
            .as_ref()
            .and_then(|s| s.audio_codec.as_deref())
        {
            Some("opus") | None => "libopus",
            Some(other) => {
                warn!("using given encoder as string directly");
                other
            }
        };
        cmd.args([
            "-c:a",
            a_encoder,
            "-b:a",
            "512k",
            "-ac",
            "6",
            "-mapping_family",
            "1",
        ])
        .arg(output_path.clone());
        debug!("{}: Running: {:?}", self, cmd);
        let output = cmd
            .output()
            .context("FFmpeg failed to start. Is it installed?")?;

        if output.status.success() {
            Ok(output_path)
        } else {
            debug!("{}", String::from_utf8_lossy(&output.stderr));
            debug!("{}", String::from_utf8_lossy(&output.stdout));
            Err(anyhow::anyhow!("FFmpeg exited with error"))
        }
    }
    pub fn upload_job_output(&self, job: &Job, output_path: &Path) -> Result<()> {
        let _guard = self.client_sems.upload.access();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let file = tokio::fs::File::open(output_path)
                .await
                .with_context(|| format!("failed to open {}", output_path.display()))?;
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = reqwest::Body::wrap_stream(stream);
            let client = reqwest::Client::new();
            let upload_url = Url::parse(&format!(
                "http://{}{}",
                self.config
                    .server_base_url
                    .host()
                    .context("invaild MEDIA_MANAGER_SERVER_URL")?,
                &job.output_url
            ))
            .context("delivery output_url is not a valid URL")?;
            debug!("{}: uploading to: {}", self, upload_url);
            client
                .put(upload_url)
                .body(body)
                .send()
                .await
                .with_context(|| format!("failed to upload output for job {}", job.id))?
                .error_for_status()
                .with_context(|| format!("output upload rejected for job {}", job.id))?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    pub fn cleanup_job_files(&self) -> Result<()> {
        match std::fs::remove_file(format!("in{}.mkv", self.id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to remove in{}.mkv", self.id)),
        }?;
        match std::fs::remove_file(format!("out{}.mkv", self.id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to remove out{}.mkv", self.id)),
        }?;
        Ok(())
    }

    pub fn report_job_complete(&self, job: &Job) -> Result<()> {
        let body = JobCompleteRequest {
            hostname: &self.config.hostname,
            job_id: &job.id,
        };
        self.http
            .get(self.config.complete_url(&job.id))
            .json(&body)
            .send()
            .with_context(|| format!("failed to send complete callback for job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("complete callback rejected for job {}", job.id))?;
        Ok(())
    }

    pub fn report_job_failed(&self, job: &Job, error: &str) -> Result<()> {
        let body = JobFailedRequest {
            hostname: &self.config.hostname,
            job_id: &job.id,
            error,
        };
        self.http
            .get(self.config.failed_url(&job.id))
            .json(&body)
            .send()
            .with_context(|| format!("failed to send failed callback for job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("failed callback rejected for job {}", job.id))?;
        Ok(())
    }
}
