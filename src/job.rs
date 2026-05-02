use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum JobResponse {
    Direct(Job),
    Wrapped { job: Job },
}

impl JobResponse {
    pub fn into_job(self) -> Job {
        match self {
            Self::Direct(job) => job,
            Self::Wrapped { job } => job,
        }
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
    pub bitrate: Option<u64>,
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

        if parts.is_empty() {
            "no transcode spec".to_string()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobCompleteRequest<'a> {
    pub hostname: &'a str,
    pub job_id: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobFailedRequest<'a> {
    pub job_id: &'a str,
    pub error: &'a str,
}
