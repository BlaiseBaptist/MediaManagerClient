# MediaManagerClient

Rust worker client for the MediaManager server.

## Current behavior

The client:

1. polls the server for the next job
2. downloads the file from the job's `input_url`
3. stores the file under `MEDIA_MANAGER_WORK_DIR/<job.id>/`
4. runs `ffmpeg` using the transcode instructions it receives
5. writes the output file under `MEDIA_MANAGER_WORK_DIR/<job.id>/`
6. uploads the output file to `delivery.output_url`

It still does **not** call `complete` yet.

If `MEDIA_MANAGER_DEBUG_DRY_RUN` is enabled, the client claims one job, prints the FFmpeg command it would run, marks the job failed with a debug note, and exits without downloading anything.

## Environment variables

- `MEDIA_MANAGER_SERVER_URL`  
  Required. Base URL of the Django server, for example `https://localhost:8000`

- `MEDIA_MANAGER_JOB_PATH`  
  Optional. Defaults to `/api/worker/jobs/next`

- `MEDIA_MANAGER_COMPLETE_PATH`  
  Optional. Defaults to `/api/worker/jobs/{job_id}/complete`

- `MEDIA_MANAGER_FAILED_PATH`  
  Optional. Defaults to `/api/worker/jobs/{job_id}/failed`

- `MEDIA_MANAGER_WORKER_ID`  
  Optional. Defaults to `worker-<pid>`

- `MEDIA_MANAGER_POLL_INTERVAL_SECS`  
  Optional. Defaults to `5`

- `MEDIA_MANAGER_WORK_DIR`  
  Optional. Defaults to `./work`

- `MEDIA_MANAGER_AUTH_TOKEN`  
  Optional. If set, the client sends `Authorization: Bearer <token>` on requests

- `MEDIA_MANAGER_ALLOW_INSECURE_TLS`  
  Optional. If set to `1`, `true`, `yes`, or `on`, the client accepts self-signed or otherwise untrusted HTTPS certificates

- `MEDIA_MANAGER_FORCE_HTTP1`  
  Optional. Defaults to `1`. Forces the HTTP client to use HTTP/1.1, which can help if the server or proxy resets large HTTP/2 downloads

- `MEDIA_MANAGER_FFMPEG_BIN`  
  Optional. Defaults to `ffmpeg`

- `MEDIA_MANAGER_DEBUG_DRY_RUN`  
  Optional. If set to `1`, `true`, `yes`, or `on`, enables the one-shot debug mode described above

## Job claim endpoint

### Request

```http
GET /api/worker/jobs/next?worker_id=<worker_id>
```

### Responses

`204 No Content`

No job is available.

`404 Not Found`

Treated the same as no job by the client.

`200 OK`

Returns a JSON job payload.

## Job payload format

The client expects this shape:

```json
{
  "id": "job-123",
  "input_url": "http://localhost:8000/api/media/jobs/job-123/input",
  "output_url": "http://localhost:8000/api/worker/jobs/job-123/output",
  "filename": "input.mp4",
  "transcode": {
    "quality": "23",
    "video_codec": "av1",
    "audio_codec": "opus",
  },
}
```

### Required fields

- `id`  
  Job identifier. Used as the local work directory name.

- `input_url`  
  URL to download the source file from.

- `output_url`
  - where the finished file should be uploaded after FFmpeg completes

- `filename`  
  Suggested local filename for the downloaded input.

- `transcode`  
  FFmpeg instructions. If omitted, the client still accepts the job.

## Lifecycle callbacks

The client has callback methods prepared for future use.

### Complete

```http
PUT {output_url}
```

```
POST /api/worker/jobs/{job_id}/failed
Content-Type: application/json
```

Body:

```json
{
  "worker_id": "worker-123",
  "error": "ffmpeg exited with code 1"
}
```

## Notes for the server

- Unknown JSON fields are ignored by the client.
- `id` and `input_url` should be present for every job.
- The client downloads the file immediately after job claim, then runs FFmpeg, writes the output to disk, and uploads it to `delivery.output_url` when provided.
- Completion reporting is not active yet, so the server should not depend on it.
