---
id: webhooks
title: Webhooks and event notifications
---

Organizations often need notifications when registry events occur, such as when a new package version is published, dist-tags change, or user permissions are modified.

This guide explains pnpr's event architecture and shows how to integrate external webhook dispatchers and notification pipelines.

## Architecture decision

pnpr deliberately does not maintain an in-process HTTP webhook dispatch queue.

Building an internal webhook queue inside a registry server introduces operational drawbacks:

- **Delivery guarantees**: An in-memory queue inside the server loses pending webhooks if an instance crashes or restarts during a deployment. Storing persistent queue state on local disk complicates running multiple stateless pnpr replicas behind a load balancer.
- **Process stability and head-of-line blocking**: Slow or unreachable webhook destination endpoints can cause queue buildup, memory pressure, and retry storms inside the registry process, potentially degrading core package download and publish throughput.
- **Separation of concerns**: Reliable event delivery, deduplication, retry policies, and fan-out are best handled by dedicated event streaming and log shipping systems.

Instead, pnpr provides durable, structured integration points designed to feed external webhook dispatchers and message brokers.

## Integration patterns

### 1. Structured audit logs

Administrative and destructive operations in pnpr emit a structured event under the `pnpr::audit` tracing target at `info` level. They cover account, team, rule, and SCIM changes, token revocation, unpublishes, packument and maintainer updates, dist-tag changes, and image deletions. Publishes and token creation are not audited. The full list of actions and their fields is in [Admin audit logs](configuration.md#admin-audit-logs).

When configured with `log.format: json`, pnpr outputs each audit event as a single-line JSON record on `stdout`:

```json
{
  "timestamp": "2026-10-17T01:15:30.123456Z",
  "level": "INFO",
  "target": "pnpr::audit",
  "fields": {
    "action": "package.dist_tag_set",
    "actor": "alice",
    "package": "@mycompany/utils",
    "tag": "latest",
    "version": "1.2.0",
    "registry": "local",
    "message": "package dist-tag set"
  },
  "span": {
    "name": "request",
    "request_id": "c8a14b09-1a73-42e1-95be-726ff342f534",
    "client_ip": "192.168.1.50"
  }
}
```

Lightweight log shippers such as [Vector](https://vector.dev/), [Fluent Bit](https://fluentbit.io/), or [Fluentd](https://www.fluentd.org/) can filter for `target == "pnpr::audit"` and forward these events to an HTTP webhook endpoint, Slack, or a message broker (such as Kafka, RabbitMQ, or AWS SQS).

#### Example: Forwarding audit logs to a webhook with Vector

Here is a Vector configuration that extracts `pnpr::audit` events from pnpr's container logs and forwards them to a webhook URL:

```toml title="vector.toml"
[sources.pnpr_logs]
type = "docker_logs"
include_containers = ["pnpr"]

[transforms.parse_pnpr_logs]
type = "remap"
inputs = ["pnpr_logs"]
source = ". = parse_json!(.message)"

[transforms.filter_audit_events]
type = "filter"
inputs = ["parse_pnpr_logs"]
condition = '.target == "pnpr::audit"'

[sinks.webhook_dispatcher]
type = "http"
inputs = ["filter_audit_events"]
uri = "https://events.example.com/webhooks/registry"
method = "post"
encoding.codec = "json"

[sinks.webhook_dispatcher.request.headers]
Authorization = "Bearer ${WEBHOOK_SECRET}"
Content-Type = "application/json"
```

### 2. Object storage bucket notifications

When pnpr is configured with an S3-compatible backend (AWS S3, Cloudflare R2, MinIO), you can configure bucket notifications directly on your object storage bucket.

Because package tarballs are stored as immutable blobs and packuments are written as JSON documents under the bucket prefix, storage notifications provide durable, real-time event triggers:

- **AWS S3**: Configure an EventBridge or SNS/SQS notification for `s3:ObjectCreated:*` events matching the prefix of hosted packages. An AWS Lambda function or worker consumes the event and dispatches your webhook.
- **MinIO**: Use MinIO bucket event notifications to dispatch events directly to an HTTP webhook endpoint.
- **Cloudflare R2**: Use R2 Event Notifications connected to a Cloudflare Worker or queue to invoke downstream webhooks.

### 3. CI/CD post-publish steps

For publishing workflows orchestrated by continuous integration (such as GitHub Actions or GitLab CI), invoking notification webhooks immediately after `pnpm publish` provides immediate, synchronous confirmation without requiring server-side webhooks:

```yaml title=".github/workflows/release.yml"
- name: Publish to pnpr
  run: pnpm publish --registry https://registry.example.com/npm/

- name: Dispatch notification webhook
  if: success()
  run: |
    curl -X POST https://events.example.com/webhooks/published \
      -H "Content-Type: application/json" \
      -d '{"package": "${{ github.repository }}", "ref": "${{ github.ref_name }}"}'
```
