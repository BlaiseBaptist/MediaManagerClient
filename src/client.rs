use crate::{
    config::Config,
    job::{Job, JobCompleteRequest, JobFailedRequest, JobResponse},
};
use anyhow::{Context, Result};
use gstrav1e;
use gstreamer::prelude::*;
use reqwest::{Body, Client, StatusCode, header};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;
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

        let builder = Client::builder()
            .default_headers(headers)
            .danger_accept_invalid_certs(config.allow_insecure_tls)
            .timeout(Duration::from_secs(300));

        let http = builder.build().context("failed to build HTTP client")?;

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

        let final_path = job_dir.join("in.mkv");
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

        println!("DEBUG:  getting file at {}", job.input_url);
        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed while streaming file")?
        {
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed to write {}", temp_path.display()))?;
        }

        println!("DEBUG: recieved file at {}", job.input_url);
        file.flush()
            .await
            .with_context(|| format!("failed to flush {}", temp_path.display()))?;

        fs::rename(&temp_path, &final_path)
            .await
            .with_context(|| format!("failed to finalize {}", final_path.display()))?;

        Ok(final_path)
    }
    fn get_best_av1_encoder() -> String {
        let registry = gstreamer::Registry::get();

        // Priority: Hardware (VAAPI) -> SVT (Fast) -> rav1e (Rust/Safe) -> AOM (Reference)
        let candidates = ["vaapiav1enc", "svtav1enc", "rav1enc", "avenc_av1"];

        for name in candidates {
            if registry
                .find_feature(name, gstreamer::ElementFactory::static_type())
                .is_some()
            {
                return name.to_string();
            }
        }

        // Fallback or error if none found
        "rav1enc".to_string()
    }
    pub async fn transcode_job_file(&self, job: &Job, input_path: &Path) -> Result<PathBuf> {
        gstreamer::init()?;
        gstrav1e::plugin_register_static().expect("Failed to bundle rav1e plugin");
        let output_path = input_path.with_file_name("out.mkv");
        let spec = job.transcode.as_ref();
        // Map video codec: default to rav1e (av1enc)
        let v_encoder = match spec.and_then(|s| s.video_codec.as_deref()) {
            Some("av1") | None => &Self::get_best_av1_encoder(),
            Some(other) => other, // Try to use the string directly if provided
        };

        // Map audio codec: default to opus
        let a_encoder = match spec.and_then(|s| s.audio_codec.as_deref()) {
            Some("opus") | None => "opusenc",
            Some(other) => other,
        };
        let quality_settings = match v_encoder {
            "vaapiav1enc" => "target-usage=4", // VAAPI uses target-usage (1-7)
            "svtav1enc" => "preset=8 crf=30",
            "rav1enc" => "speed-preset=4 quantizer=80",
            "avenc_av1" => "cpu-used=4", // libaom settings
            _ => "",
        };
        // 4. Construct the Pipeline String
        // We use 'decodebin' to handle any input format and 'matroskamux' for the .mkv container
        let pipeline_str = format!(
            "filesrc location=\"{}\" ! decodebin force-sw-decoders=true name=dbin \
         matroskamux name=mux ! filesink location=\"{}\" \
         dbin. ! queue ! videoconvert ! {} {} ! mux. \
         dbin. ! queue ! audioconvert ! audioresample ! {} ! mux.",
            input_path.to_str().context("Invalid input path")?,
            output_path.to_str().context("Invalid output path")?,
            v_encoder,
            quality_settings,
            a_encoder
        );

        // 5. Run the Pipeline
        let pipeline = gstreamer::parse::launch(&pipeline_str)
            .context("Failed to parse gstreamerreamer pipeline string")?;

        pipeline.set_state(gstreamer::State::Playing)?;

        // 6. Wait for Completion (EOS) or Error
        let bus = pipeline.bus().context("Failed to get pipeline bus")?;

        // In a real async app, you'd use a stream, but for a job file, a sync loop is fine
        for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
            use gstreamer::MessageView;
            match msg.view() {
                MessageView::Eos(..) => break,
                MessageView::Error(err) => {
                    pipeline.set_state(gstreamer::State::Null)?;
                    return Err(anyhow::anyhow!(
                        "GStreamer Error: {} ({})",
                        err.error(),
                        err.debug().unwrap_or_else(|| "no debug info".into())
                    ));
                }
                _ => (),
            }
        }

        pipeline.set_state(gstreamer::State::Null)?;
        Ok(output_path)
    }

    pub async fn upload_job_output(&self, job: &Job, output_path: &Path) -> Result<()> {
        let file = fs::File::open(output_path)
            .await
            .with_context(|| format!("failed to open {}", output_path.display()))?;
        let stream = ReaderStream::new(file);
        let body = Body::wrap_stream(stream);

        self.http
            .post(Url::parse(&job.output_url).context("delivery output_url is not a valid URL")?)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", job.filename),
            )
            .header("X-Output-Filename", job.filename.clone())
            .body(body)
            .send()
            .await
            .with_context(|| format!("failed to upload output for job {}", job.id))?
            .error_for_status()
            .with_context(|| format!("output upload rejected for job {}", job.id))?;
        Ok(())
    }

    pub async fn cleanup_job_files(&self, job: &Job) -> Result<()> {
        let job_dir = self.config.work_dir.join(&job.id);
        match fs::remove_dir_all(&job_dir).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to remove {}", job_dir.display())),
        }
    }

    #[allow(dead_code)]
    pub async fn report_job_complete(&self, job: &Job) -> Result<()> {
        let body = JobCompleteRequest {
            worker_id: &self.config.worker_id,
            output_url: &job.output_url,
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
}
