---
"@pnpm/pnpr": minor
---

pnpr can keep hosted package metadata in a libSQL database, such as Turso, shared by several pnpr instances. Set `metadata.url` to a `libsql://` URL. It works with an `s3:` block, so archives stay in object storage. With `metadata.replicaPath`, each instance reads package documents from a local replica that syncs every `metadata.syncInterval`. Permission checks and the reads a write depends on always read the primary.
