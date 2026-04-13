# MediaManagerClient

Rust worker client for the MediaManager server.

## Current behavior

The client:

1. polls the server for the next job
2. downloads the file from the job's `input_url`
3. stores the file under `MEDIA_MANAGER_WORK_DIR/<job.id>/`
4. runs `ffmpeg` using the transcode instructions it receives
5. writes the output file under `MEDIA_MANAGER_WORK_DIR/<job.id>/`

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
  "filename": "input.mp4",
  "transcode": {
    "quality": "23",
    "video_codec": "libx264",
    "audio_codec": "aac",
    "ffmpeg_args": ["-preset", "slow", "-movflags", "+faststart"]
  },
  "delivery": {
    "output_url": "http://localhost:8000/api/worker/jobs/job-123/output",
    "filename": "output.mp4"
  }
}
```

### Required fields

- `id`  
  Job identifier. Used as the local work directory name.

- `input_url`  
  URL to download the source file from.

### Optional fields

- `filename`  
  Suggested local filename for the downloaded input.

- `transcode`  
  FFmpeg instructions. If omitted, the client still accepts the job.

- `delivery`  
  Output target instructions. If omitted, the client still accepts the job.

## `transcode` block

The client uses these values to build the FFmpeg command.

```json
{
  "quality": "23",
  "video_codec": "libx264",
  "audio_codec": "aac",
  "ffmpeg_args": ["-preset", "slow"]
}
```

- `quality`
  - free-form quality selector
  - can map to CRF, bitrate, or any server-side convention you choose

- `video_codec`
  - example values: `libx264`, `libx265`, `h264_nvenc`

- `audio_codec`
  - example values: `aac`, `libopus`, `copy`

- `ffmpeg_args`
  - extra FFmpeg CLI arguments in order
  - example: `["-preset", "slow", "-movflags", "+faststart"]`

## `delivery` block

The client uses these values to choose the output filename and logs them for visibility.

```json
{
  "output_url": "http://localhost:8000/api/worker/jobs/job-123/output",
  "filename": "output.mp4"
}
```

- `output_url`
  - where the finished file should eventually be sent

- `filename`
  - suggested filename for the final output artifact

## Lifecycle callbacks

The client has callback methods prepared for future use.

### Complete

```http
POST /api/worker/jobs/{job_id}/complete
Content-Type: application/json
```

Body:

```json
{
  "worker_id": "worker-123",
  "output_url": "http://localhost:8000/api/worker/jobs/job-123/output"
}
```

`output_url` is optional.

### Failed

```http
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
- The client downloads the file immediately after job claim, then runs FFmpeg and writes the output to disk.
- Completion reporting is not active yet, so the server should not depend on it.
