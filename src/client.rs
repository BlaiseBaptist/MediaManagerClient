use crate::{
    config::Config,
    job::{Job, JobCompleteRequest, JobFailedRequest, JobResponse},
};
use anyhow::{Context, Result};
use gstreamer::prelude::*;
use reqwest::{StatusCode, blocking::Client};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::{fs, io::AsyncWriteExt};
use url::Url;

pub struct ServerClient {
    http: Client,
    config: Config,
}

impl ServerClient {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::new(),
            config,
        })
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
        let job_dir = self.config.work_dir.join(&job.id);
        std::fs::create_dir_all(&job_dir)
            .with_context(|| format!("failed to create work dir {}", job_dir.display()))?;
        let final_path = job_dir.join("in.mkv");
        let temp_path = final_path.with_extension("part");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut response = reqwest::get(
                Url::parse(&job.input_url).context("job input_url is not a valid URL")?,
            )
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
        let output = Command::new("ffmpeg").arg("-encoders").output();

        let Ok(output) = output else {
            return "librav1e".to_string();
        };

        let encoder_list = String::from_utf8_lossy(&output.stdout);

        let candidates = ["av1_nvenc", "av1_qsv", "av1_vaapi", "libsvtav1", "librav1e"];

        for name in candidates {
            if encoder_list.contains(name) {
                return name.to_string();
            }
        }

        "librav1e".to_string()
    }

    pub fn transcode_job_file(&self, job: &Job, input_path: &Path) -> Result<PathBuf> {
        let output_path = input_path.with_file_name("out.mkv");

        // Determine encoder (matches your logic)
        let v_encoder = match job
            .transcode
            .as_ref()
            .and_then(|s| s.video_codec.as_deref())
        {
            Some("av1") | None => &Self::get_best_av1_encoder(), // Standard FFmpeg name
            Some(other) => other,
        };

        let mut cmd = Command::new("ffmpeg");

        cmd.arg("-y") // Overwrite output
            .arg("-i")
            .arg(input_path)
            .arg("-c:v")
            .arg(v_encoder);

        // Apply your specific settings
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
            Some(other) => other,
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
        println!("running: {:?}", cmd);
        let status = cmd
            .status()
            .context("FFmpeg failed to start. Is it installed?")?;

        if status.success() {
            Ok(output_path)
        } else {
            Err(anyhow::anyhow!("FFmpeg exited with error"))
        }
    }
    fn get_best_gs_av1_encoder() -> String {
        let registry = gstreamer::Registry::get();
        let candidates = ["vaapiav1enc", "svtav1enc", "rav1enc", "avenc_av1"];
        for name in candidates {
            if registry
                .find_feature(name, gstreamer::ElementFactory::static_type())
                .is_some()
            {
                return name.to_string();
            }
        }
        "rav1enc".to_string()
    }
    #[allow(dead_code)]
    pub fn gs_transcode_job_file(&self, job: &Job, input_path: &Path) -> Result<PathBuf> {
        let abs_input = input_path.canonicalize()?;
        let output_path = input_path.with_file_name("out.mkv");

        let a_encoder = match job
            .transcode
            .as_ref()
            .and_then(|s| s.audio_codec.as_deref())
        {
            Some("opus") | None => "opusenc",
            Some(other) => other,
        };
        let v_encoder = match job
            .transcode
            .as_ref()
            .and_then(|s| s.video_codec.as_deref())
        {
            Some("av1") | None => &Self::get_best_gs_av1_encoder(),
            Some(other) => other,
        };
        let v_settings = match v_encoder {
            "svtav1enc" => "preset=4 crf=22",
            "rav1enc" => "speed-preset=3 quantizer=70 threads=0 tiles=9",
            "avenc_av1" => "cpu-used=3 row-mt=true threads=16",
            _ => "",
        };
        let pipeline_str = format!(
            "uridecodebin3 uri=file://{input_path} name=dbin \
            matroskamux name=mux ! filesink location=\"{output_path}\" \
            dbin. ! queue name=v_queue max-size-buffers=5000 max-size-bytes=0 max-size-time=0 ! \
            videoconvert ! video/x-raw,format=I420_10LE ! \
            {v_enc} {v_settings} ! queue ! mux. \
            dbin. ! queue name=a_queue max-size-buffers=0 max-size-bytes=0 max-size-time=0 ! \
            audioconvert ! audioresample ! {a_enc} ! queue ! mux.",
            input_path = abs_input.display(),
            output_path = output_path.display(),
            v_enc = v_encoder,
            v_settings = v_settings,
            a_enc = a_encoder
        );
        let pipeline =
            gstreamer::parse::launch(&pipeline_str).context("Failed to parse pipeline")?;
        pipeline.set_state(gstreamer::State::Playing)?;

        let bus = pipeline.bus().context("No bus")?;
        for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
            use gstreamer::MessageView;
            match msg.view() {
                MessageView::Eos(..) => break,
                MessageView::Error(err) => {
                    pipeline.set_state(gstreamer::State::Null)?;
                    return Err(anyhow::anyhow!("GStreamer Error: {}", err.error()));
                }
                _ => (),
            }
        }

        pipeline.set_state(gstreamer::State::Null)?;
        Ok(output_path)
    }

    pub fn upload_job_output(&self, job: &Job, output_path: &Path) -> Result<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let file = tokio::fs::File::open(output_path)
                .await
                .with_context(|| format!("failed to open {}", output_path.display()))?;
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = reqwest::Body::wrap_stream(stream);
            let client = reqwest::Client::new();
            client
                .put(
                    Url::parse(&job.output_url)
                        .context("delivery output_url is not a valid URL")?,
                )
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

    pub fn cleanup_job_files(&self, job: &Job) -> Result<()> {
        let job_dir = self.config.work_dir.join(&job.id);
        match std::fs::remove_dir_all(&job_dir) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to remove {}", job_dir.display())),
        }
    }

    #[allow(dead_code)]
    pub fn report_job_complete(&self, job: &Job) -> Result<()> {
        let body = JobCompleteRequest { job_id: &job.id };
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
