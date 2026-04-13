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
    pub filename: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct JobCompleteRequest<'a> {
    pub worker_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_url: Option<&'a str>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct JobFailedRequest<'a> {
    pub worker_id: &'a str,
    pub error: &'a str,
}
