use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum JobResponse {
    Direct(Job),
    Wrapped { job: Job },
}

impl JobResponse {
    pub fn into_job(self) -> Job {
        let mut job = match self {
            Self::Direct(job) => job,
            Self::Wrapped { job } => job,
        };
        job.filename = sanitize_filename(&job.filename);
        return job;
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub id: String,
    pub input_url: String,
    #[serde(default)]
    pub transcode: Option<TranscodeSpec>,
    #[serde(default)]
    pub output_url: String,
    #[serde(default)]
    pub filename: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranscodeSpec {
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub ffmpeg_args: Vec<String>,
}

impl TranscodeSpec {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(value) = &self.quality {
            parts.push(format!("quality={value}"));
        }
        if let Some(value) = &self.video_codec {
            parts.push(format!("video_codec={value}"));
        }
        if let Some(value) = &self.audio_codec {
            parts.push(format!("audio_codec={value}"));
        }
        if !self.ffmpeg_args.is_empty() {
            parts.push(format!("ffmpeg_args={}", self.ffmpeg_args.join(" ")));
        }

        if parts.is_empty() {
            "no transcode spec".to_string()
        } else {
            parts.join(", ")
        }
    }
}

impl Job {
    pub fn planned_input_path(&self, work_dir: &std::path::Path) -> std::path::PathBuf {
        work_dir.join(&self.id).join(self.filename.clone())
    }

    pub fn planned_output_path(&self, work_dir: &std::path::Path) -> std::path::PathBuf {
        work_dir
            .join(&self.id)
            .join(self.planned_staging_output_filename())
    }

    pub fn planned_staging_output_filename(&self) -> String {
        let delivery_filename = self.filename.clone();
        if let Some((stem, extension)) = delivery_filename.rsplit_once('.') {
            format!("{stem}.transcoded.{extension}")
        } else {
            format!("{delivery_filename}.transcoded")
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct JobCompleteRequest<'a> {
    pub worker_id: &'a str,
    pub output_url: &'a str,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct JobFailedRequest<'a> {
    pub worker_id: &'a str,
    pub error: &'a str,
}

fn sanitize_filename(name: &str) -> String {
    let trimmed = std::path::Path::new(name)
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
