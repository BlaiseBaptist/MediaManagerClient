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

## Environment variables

- `MEDIA_MANAGER_SERVER_URL`

  Required. Base URL of the Django server, for example `https://localhost:8000`
- `MEDIA_MANAGER_JOB_PATH`

  Optional. Defaults to `/api/worker/jobs/next`
- `MEDIA_MANAGER_WORK_DIR`

  Optional. Default to system tmp dir
- `MEDIA_MANAGER_COMPLETE_PATH`

  Optional. Defaults to `/api/worker/jobs/{job_id}/complete`
- `MEDIA_MANAGER_FAILED_PATH`

  Optional. Defaults to `/api/worker/jobs/{job_id}/failed`
- `MEDIA_MANAGER_WORKER_ID`

  Optional. Defaults to `worker-<pid>`
- `MEDIA_MANAGER_POLL_INTERVAL_SECS`

  Optional. Defaults to `5`
- `MEDIA_MANAGER_FFMPEG_BIN`

  Optional. Defaults to `ffmpeg`
- `MEDIA_MANAGER_DOWNLOADS`

  Optional. Defaults to `1`
- `MEDIA_MANAGER_UPLOADS`

  Optional. Defaults to `1`
- `MEDIA_MANAGER_TRANSCODES`

  Optional. Defaults to `2`
- `HOSTNAME`

  Optional. Defaults to system hostname

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
  "id": "123",
  "input_url": "http://localhost:8000/api/media/jobs/job-123/input",
  "output_url": "http://localhost:8000/api/worker/jobs/job-123/output",
  "transcode": {
    "quality": "HIGH",
    "video_codec": "av1",
    "audio_codec": "opus"
  }
}
```

### Fields

- `id`\
  Job identifier. Used as the local work directory name.
- `input_url`
- `output_url`
- `transcode`
