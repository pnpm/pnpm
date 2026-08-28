---
"@pnpm/pnpr": minor
---

Added an experimental pipeline run-record surface, off by default and enabled with `pipeline.enabled: true`: `pnpm pipeline --report` (or `--report-to <url>`) publishes a run's summary and event stream to `PUT /-/pnpr/v0/pipeline/runs`, runs are append-only and listed at `GET /-/pnpr/v0/pipeline/runs` (with `?workspace=` and `?limit=`), a single run's full record including its events is served at `GET /-/pnpr/v0/pipeline/runs/{workspace}/{runId}`, and `GET /-/pnpr/v0/pipeline` serves a small viewer page over those endpoints.
