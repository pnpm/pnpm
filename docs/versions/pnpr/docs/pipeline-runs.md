---
id: pipeline-runs
title: Pipeline run records
---

Added in: v0.1.0-alpha.11

pnpr can store the reports [`pnpm pipeline`](/cli/pipeline) produces, so a
team's CI runs are listed and served from one place instead of living in the
scrollback of whichever machine ran them.

```yaml
pipeline:
  enabled: true
  workspaces:
    acme-app:
      access: [team:platform]
      publish: ci-builder
```

`access` decides who may list and read a workspace's runs, `publish` who may
submit one. A workspace nobody may access is invisible rather than forbidden: a
listing filters to what the caller can see, and a read of an unreadable
workspace answers `404`.

The workspace key is the identity `pnpm pipeline` derives from the workspace
root directory. Run the command once with `--report` and read the name back from
the listing.

Records live with the hosted packages, so a run submitted through one replica is
listed and served by every other.

## Submitting a run

```sh
pnpm pipeline --report
pnpm pipeline --report-to https://runs.example.com
```

`--report` submits to the server named by
[`pnprServer`](install-acceleration.md#enabling-it), which also drives install
offloading. `--report-to` names a different server, which is the better spelling
when the server that stores runs is not the one that accelerates installs. A
failed run is submitted before the command exits non-zero.

Each submission carries a summary document and the run's event stream.

## Reading runs back

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/pipeline/runs` | Record one run. Requires `publish` on the run's workspace. |
| `GET` | `/-/pnpr/v0/pipeline/runs` | The most recent run summaries, newest first. Accepts `workspace` and `limit`. |
| `GET` | `/-/pnpr/v0/pipeline/runs/{workspace}/{run_id}` | One run's full record, event stream included. |
| `GET` | `/-/pnpr/v0/pipeline` | A self-contained web viewer over the two read endpoints. |

The viewer is static HTML with no data of its own. The reads it issues carry the
token the visitor pastes, so the page itself needs no authentication.
