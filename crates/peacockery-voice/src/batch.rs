//! Batch transcription: upload (single or multipart PUT to signed URLs), job creation, polling.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{CONTENT_LENGTH, ETAG};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::sync::CancellationToken;

use crate::artifact::Artifact;
use crate::catalog::ModelFamily;
use crate::client::{check, decode, VoiceClient, JSON_TIMEOUT};
use crate::error::{Error, Result};

/// Progress chunk for upload callbacks.
const CHUNK: usize = 256 * 1024;
/// PUT attempts per transfer (renewals and transient retries share the budget).
const PUT_ATTEMPTS: u32 = 4;
/// Consecutive failed polls tolerated before giving up.
const POLL_FAILURES: u32 = 6;

/// Progress reported while a file is transcribed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum BatchProgress {
    Uploading {
        sent: u64,
        total: u64,
    },
    Uploaded {
        upload_id: String,
        input_key: String,
    },
    JobCreated {
        job_id: String,
    },
    /// Every poll. `error` carries transient provider retry text while the job is re-queued.
    Job {
        job_id: String,
        status: JobStatus,
        progress: Option<f64>,
        error: Option<String>,
    },
}

/// Progress callback. Called from the task running the operation, possibly from inside the upload body stream.
pub type ProgressFn = Arc<dyn Fn(BatchProgress) + Send + Sync>;

/// `POST /v1/uploads` / `…/transfer` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadReservation {
    pub upload_id: String,
    pub input_key: String,
    pub transfer: Transfer,
}

/// How to send the bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Transfer {
    Single {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    Multipart {
        part_size_bytes: u64,
        parts: Vec<TransferPart>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferPart {
    pub part_number: u32,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// A part ETag for `complete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedPart {
    pub part_number: u32,
    pub etag: String,
}

/// A finished upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedUpload {
    pub upload_id: String,
    pub input_key: String,
    pub size_bytes: u64,
}

/// Job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    #[serde(other)]
    Unknown,
}



/// `GET /v1/jobs/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobView {
    pub job_id: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub status: JobStatus,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_details: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// Options for a transcription job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscribeOptions {
    /// Batch model id (`elevenlabs-scribe_v2`, `mistral-voxtral-small-transcribe-3`, …).
    pub asr_model: String,
    pub diarize: bool,
    /// ISO-639-1; `None` or `"auto"` lets the provider detect.
    pub language: Option<String>,
    /// Sent as `provider_options.elevenlabs.keyterms` / `.mistral.contextBias` / `.deepgram.keyterm`.
    pub keyterms: Vec<String>,
    /// `Idempotency-Key` for job creation (a replay returns the existing job).
    pub idempotency_key: Option<String>,
}

impl TranscribeOptions {
    pub fn new(asr_model: impl Into<String>) -> Self {
        Self {
            asr_model: asr_model.into(),
            diarize: true,
            language: Some("en".into()),
            keyterms: Vec::new(),
            idempotency_key: None,
        }
    }

    /// `POST /v1/transcriptions` body for an uploaded input.
    pub fn request_body(&self, input_key: &str) -> Value {
        let mut body =
            json!({ "asr_model": self.asr_model, "input_key": input_key, "diarize": self.diarize });
        if let Some(lang) = self
            .language
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty() && *l != "auto")
        {
            body["language"] = json!(lang);
        }
        let mut terms: Vec<&str> = Vec::new();
        for t in self
            .keyterms
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
        {
            if !terms.contains(&t) {
                terms.push(t);
            }
        }
        if !terms.is_empty() {
            let options = match ModelFamily::of(&self.asr_model) {
                ModelFamily::Scribe => Some(json!({ "elevenlabs": { "keyterms": terms } })),
                ModelFamily::Mistral | ModelFamily::VoxtralRealtime3 => {
                    Some(json!({ "mistral": { "contextBias": terms } }))
                }
                ModelFamily::Deepgram => Some(json!({ "deepgram": { "keyterm": terms } })),
                ModelFamily::Cohere | ModelFamily::Other => None,
            };
            if let Some(options) = options {
                body["provider_options"] = options;
            }
        }
        body
    }
}

/// Result of [`VoiceClient::transcribe_file`].
#[derive(Debug, Clone, PartialEq)]
pub struct BatchTranscript {
    pub upload_id: String,
    pub job_id: String,
    pub artifact: Artifact,
}

/// Upload content type from the file extension (the server accepts only these).
pub fn content_type_for(path: &Path) -> Result<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Ok(match ext.as_str() {
        "mp4" | "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "webm" => "audio/webm",
        "mov" => "video/quicktime",
        _ => {
            return Err(Error::Config(format!(
                "unsupported audio file type: {}",
                path.display()
            )))
        }
    })
}

/// Poll delay after `n` polls: 2 s for the first three, then +1 s per poll up to 10 s.
pub fn poll_delay(n: u32) -> Duration {
    Duration::from_secs(u64::from((2 + n.saturating_sub(2)).min(10)))
}

impl VoiceClient {
    /// `POST /v1/uploads`.
    pub async fn create_upload(
        &self,
        content_type: &str,
        filename: &str,
        size_bytes: u64,
    ) -> Result<UploadReservation> {
        self.post_json("v1/uploads", &json!({ "content_type": content_type, "filename": filename, "size_bytes": size_bytes }))
            .await
    }

    /// `POST /v1/uploads/{id}/transfer`: fresh signed URL(s) after expiry (`part_number` for multipart).
    pub async fn renew_transfer(
        &self,
        upload_id: &str,
        part_number: Option<u32>,
    ) -> Result<UploadReservation> {
        let body = match part_number {
            Some(n) => json!({ "part_number": n }),
            None => json!({}),
        };
        self.post_json(&format!("v1/uploads/{upload_id}/transfer"), &body)
            .await
    }

    /// `POST /v1/uploads/{id}/complete`.
    pub async fn complete_upload(
        &self,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<CompletedUpload> {
        #[derive(Deserialize)]
        struct Done {
            input_key: String,
            #[serde(default)]
            size_bytes: u64,
        }
        let done: Done = self
            .post_json(
                &format!("v1/uploads/{upload_id}/complete"),
                &json!({ "parts": parts }),
            )
            .await?;
        Ok(CompletedUpload {
            upload_id: upload_id.to_string(),
            input_key: done.input_key,
            size_bytes: done.size_bytes,
        })
    }

    /// Upload a local file: reserve, PUT (single or multipart with ETags, renewing expired URLs on 403), complete.
    pub async fn upload_file(
        &self,
        path: &Path,
        cancel: &CancellationToken,
        progress: &ProgressFn,
    ) -> Result<CompletedUpload> {
        let content_type = content_type_for(path)?;
        let size = tokio::fs::metadata(path).await?.len();
        if size == 0 {
            return Err(Error::Config(format!("{} is empty", path.display())));
        }
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("audio");
        let reservation = self.create_upload(content_type, filename, size).await?;
        let upload_id = reservation.upload_id.clone();
        progress(BatchProgress::Uploading {
            sent: 0,
            total: size,
        });

        let parts = match reservation.transfer {
            Transfer::Single { url, headers } => {
                let bytes = Bytes::from(tokio::fs::read(path).await?);
                if bytes.len() as u64 != size {
                    return Err(Error::Config(format!(
                        "{} changed while uploading",
                        path.display()
                    )));
                }
                let target = PutTarget {
                    url,
                    headers,
                    part_number: None,
                };
                self.put_with_retry(&upload_id, target, bytes, 0, size, cancel, progress)
                    .await?;
                Vec::new()
            }
            Transfer::Multipart {
                part_size_bytes,
                mut parts,
            } => {
                if part_size_bytes == 0 || parts.is_empty() {
                    return Err(Error::Decode("multipart reservation without parts".into()));
                }
                parts.sort_by_key(|p| p.part_number);
                let mut file = tokio::fs::File::open(path).await?;
                let mut done = Vec::with_capacity(parts.len());
                for part in parts {
                    let offset = u64::from(part.part_number.saturating_sub(1)) * part_size_bytes;
                    let len = part_size_bytes.min(size.saturating_sub(offset));
                    if len == 0 {
                        return Err(Error::Decode(format!(
                            "part {} lies beyond the file",
                            part.part_number
                        )));
                    }
                    file.seek(std::io::SeekFrom::Start(offset)).await?;
                    let mut buf = vec![0u8; len as usize];
                    file.read_exact(&mut buf).await?;
                    let target = PutTarget {
                        url: part.url,
                        headers: part.headers,
                        part_number: Some(part.part_number),
                    };
                    let etag = self
                        .put_with_retry(
                            &upload_id,
                            target,
                            Bytes::from(buf),
                            offset,
                            size,
                            cancel,
                            progress,
                        )
                        .await?;
                    let etag = etag.ok_or_else(|| {
                        Error::Decode(format!("part {} response has no ETag", part.part_number))
                    })?;
                    done.push(CompletedPart {
                        part_number: part.part_number,
                        etag,
                    });
                }
                done
            }
        };
        progress(BatchProgress::Uploading {
            sent: size,
            total: size,
        });
        let completed = self.complete_upload(&upload_id, &parts).await?;
        progress(BatchProgress::Uploaded {
            upload_id: upload_id.clone(),
            input_key: completed.input_key.clone(),
        });
        Ok(completed)
    }

    #[allow(clippy::too_many_arguments)]
    async fn put_with_retry(
        &self,
        upload_id: &str,
        mut target: PutTarget,
        bytes: Bytes,
        offset: u64,
        total: u64,
        cancel: &CancellationToken,
        progress: &ProgressFn,
    ) -> Result<Option<String>> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let put = self.put_once(&target, bytes.clone(), offset, total, progress.clone());
            let outcome = tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                r = put => r,
            };
            match outcome {
                Ok(etag) => return Ok(etag),
                Err(Error::Forbidden(body)) if attempt < PUT_ATTEMPTS => {
                    tracing::info!(upload_id, part = ?target.part_number, %body, "transfer URL rejected; renewing");
                    let renewed = self.renew_transfer(upload_id, target.part_number).await?;
                    target = target.renewed(renewed.transfer)?;
                }
                Err(err) if err.is_transient() && attempt < PUT_ATTEMPTS => {
                    tracing::warn!(upload_id, error = %err, attempt, "upload PUT failed; retrying");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                        _ = tokio::time::sleep(Duration::from_secs(1 << attempt)) => {}
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn put_once(
        &self,
        target: &PutTarget,
        bytes: Bytes,
        offset: u64,
        total: u64,
        progress: ProgressFn,
    ) -> Result<Option<String>> {
        let len = bytes.len();
        let chunks = (0..len.div_ceil(CHUNK)).map(move |i| {
            let end = ((i + 1) * CHUNK).min(len);
            let chunk = bytes.slice(i * CHUNK..end);
            progress(BatchProgress::Uploading {
                sent: offset + end as u64,
                total,
            });
            Ok::<Bytes, std::io::Error>(chunk)
        });
        let mut request = self
            .transfer()
            .put(&target.url)
            .header(CONTENT_LENGTH, len)
            .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                chunks,
            )));
        for (name, value) in &target.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = check(request.send().await?).await?;
        Ok(response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string))
    }

    /// `POST /v1/transcriptions`.
    pub async fn create_transcription(
        &self,
        input_key: &str,
        options: &TranscribeOptions,
    ) -> Result<JobView> {
        let mut request = self
            .api()
            .post(self.url("v1/transcriptions")?)
            .timeout(JSON_TIMEOUT)
            .json(&options.request_body(input_key));
        if let Some(key) = options
            .idempotency_key
            .as_deref()
            .filter(|k| !k.trim().is_empty())
        {
            request = request.header("idempotency-key", key.trim());
        }
        decode(request.send().await?).await
    }

    /// `GET /v1/jobs/{id}`.
    pub async fn get_job(&self, job_id: &str) -> Result<JobView> {
        self.get_json(&format!("v1/jobs/{job_id}")).await
    }

    /// Poll a job (2 s, backing off to 10 s) until it succeeds, fails or `cancel` fires.
    pub async fn wait_for_job(
        &self,
        job_id: &str,
        cancel: &CancellationToken,
        progress: &ProgressFn,
    ) -> Result<Artifact> {
        let mut polls = 0;
        let mut failures = 0;
        loop {
            match self.get_job(job_id).await {
                Ok(job) => {
                    failures = 0;
                    progress(BatchProgress::Job {
                        job_id: job_id.to_string(),
                        status: job.status,
                        progress: job.progress,
                        error: job.error.clone(),
                    });
                    match job.status {
                        JobStatus::Succeeded => {
                            let result = job.result.ok_or_else(|| {
                                Error::Decode(format!("job {job_id} succeeded without a result"))
                            })?;
                            return Ok(Artifact::from_value(result)?);
                        }
                        JobStatus::Failed => {
                            return Err(Error::JobFailed {
                                job_id: job_id.to_string(),
                                message: job.error.unwrap_or_else(|| "transcription failed".into()),
                                details: job.error_details,
                            });
                        }
                        _ => {}
                    }
                }
                Err(err) if err.is_transient() && failures + 1 < POLL_FAILURES => {
                    failures += 1;
                    tracing::warn!(job_id, error = %err, failures, "job poll failed; retrying");
                }
                Err(err) => return Err(err),
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                _ = tokio::time::sleep(poll_delay(polls)) => {}
            }
            polls += 1;
        }
    }

    /// Upload a file, create the job and wait for its artifact.
    pub async fn transcribe_file(
        &self,
        path: &Path,
        options: &TranscribeOptions,
        cancel: &CancellationToken,
        progress: ProgressFn,
    ) -> Result<BatchTranscript> {
        let upload = self.upload_file(path, cancel, &progress).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let job = self
            .create_transcription(&upload.input_key, options)
            .await?;
        progress(BatchProgress::JobCreated {
            job_id: job.job_id.clone(),
        });
        let artifact = self.wait_for_job(&job.job_id, cancel, &progress).await?;
        Ok(BatchTranscript {
            upload_id: upload.upload_id,
            job_id: job.job_id,
            artifact,
        })
    }
}

struct PutTarget {
    url: String,
    headers: BTreeMap<String, String>,
    part_number: Option<u32>,
}

impl PutTarget {
    fn renewed(self, transfer: Transfer) -> Result<Self> {
        match (self.part_number, transfer) {
            (None, Transfer::Single { url, headers }) => Ok(PutTarget {
                url,
                headers,
                part_number: None,
            }),
            (Some(n), Transfer::Multipart { parts, .. }) => {
                let part = parts
                    .into_iter()
                    .find(|p| p.part_number == n)
                    .ok_or_else(|| Error::Decode(format!("renewal did not return part {n}")))?;
                Ok(PutTarget {
                    url: part.url,
                    headers: part.headers,
                    part_number: Some(n),
                })
            }
            _ => Err(Error::Decode(
                "renewed transfer kind does not match the upload".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_options_per_family() {
        let mut o = TranscribeOptions::new("elevenlabs-scribe_v2");
        o.keyterms = vec![
            "Cica-Huntek".into(),
            "Netask".into(),
            "Netask".into(),
            " ".into(),
        ];
        assert_eq!(
            o.request_body("uploads/u/source"),
            json!({"asr_model":"elevenlabs-scribe_v2","input_key":"uploads/u/source","diarize":true,"language":"en",
                   "provider_options":{"elevenlabs":{"keyterms":["Cica-Huntek","Netask"]}}})
        );
        o.asr_model = "mistral-voxtral-small-transcribe-3".into();
        assert_eq!(
            o.request_body("k")["provider_options"],
            json!({"mistral":{"contextBias":["Cica-Huntek","Netask"]}})
        );
        o.asr_model = "deepgram-nova-3".into();
        assert_eq!(
            o.request_body("k")["provider_options"],
            json!({"deepgram":{"keyterm":["Cica-Huntek","Netask"]}})
        );
        o.asr_model = "cohere-transcribe".into();
        o.language = Some("auto".into());
        o.diarize = false;
        assert_eq!(
            o.request_body("k"),
            json!({"asr_model":"cohere-transcribe","input_key":"k","diarize":false})
        );
        o.keyterms.clear();
        o.asr_model = "elevenlabs-scribe_v2".into();
        assert!(o.request_body("k").get("provider_options").is_none());
    }

    #[test]
    fn reservations_parse() {
        let single: UploadReservation = serde_json::from_str(
            r#"{"upload_id":"upl_1","input_key":"uploads/upl_1/source","transfer":{"kind":"single","url":"https://r2/x","headers":{"content-type":"audio/mp4"}}}"#,
        )
        .unwrap();
        assert!(
            matches!(&single.transfer, Transfer::Single { headers, .. } if headers["content-type"] == "audio/mp4")
        );
        let multi: UploadReservation = serde_json::from_str(
            r#"{"upload_id":"upl_2","input_key":"k","transfer":{"kind":"multipart","part_size_bytes":16777216,
                "parts":[{"part_number":1,"url":"https://r2/1","headers":{}},{"part_number":2,"url":"https://r2/2"}]}}"#,
        )
        .unwrap();
        let Transfer::Multipart {
            part_size_bytes,
            parts,
        } = &multi.transfer
        else {
            panic!()
        };
        assert_eq!((*part_size_bytes, parts.len()), (16_777_216, 2));

        let target = PutTarget {
            url: "old".into(),
            headers: BTreeMap::new(),
            part_number: Some(2),
        };
        let renewed = target.renewed(multi.transfer.clone()).unwrap();
        assert_eq!(renewed.url, "https://r2/2");
        let target = PutTarget {
            url: "old".into(),
            headers: BTreeMap::new(),
            part_number: None,
        };
        assert!(target.renewed(multi.transfer).is_err());
        let target = PutTarget {
            url: "old".into(),
            headers: BTreeMap::new(),
            part_number: None,
        };
        assert_eq!(target.renewed(single.transfer).unwrap().url, "https://r2/x");
        let parts = [CompletedPart {
            part_number: 1,
            etag: "\"abc\"".into(),
        }];
        assert_eq!(
            serde_json::to_value(parts).unwrap(),
            json!([{"part_number":1,"etag":"\"abc\""}])
        );
    }

    #[test]
    fn job_views_parse() {
        let path = format!(
            "{}/tests/fixtures/b-scribe.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let job: JobView = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(job.status, JobStatus::Succeeded);
        assert!(job.job_id.starts_with("job_"));
        assert!(
            Artifact::from_value(job.result.unwrap())
                .unwrap()
                .content
                .words
                .items
                .len()
                > 100
        );
        let queued: JobView = serde_json::from_str(r#"{"job_id":"j","status":"queued","progress":0,"error":"transient provider error: 503"}"#).unwrap();
        assert_eq!(queued.status, JobStatus::Queued);

        let odd: JobView = serde_json::from_str(r#"{"job_id":"j","status":"paused"}"#).unwrap();
        assert_eq!(odd.status, JobStatus::Unknown);
    }

    #[test]
    fn content_types_and_backoff() {
        assert_eq!(
            content_type_for(Path::new("a/audio.mp4")).unwrap(),
            "audio/mp4"
        );
        assert_eq!(content_type_for(Path::new("x.M4A")).unwrap(), "audio/mp4");
        assert_eq!(content_type_for(Path::new("x.flac")).unwrap(), "audio/flac");
        assert_eq!(content_type_for(Path::new("x.wav")).unwrap(), "audio/wav");
        assert!(content_type_for(Path::new("x.pcm")).is_err());
        let delays: Vec<u64> = (0..12).map(|n| poll_delay(n).as_secs()).collect();
        assert_eq!(delays, [2, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10]);
    }
}
