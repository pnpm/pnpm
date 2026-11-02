---
"@pnpm/pnpr": minor
---

pnpr now keeps hosted package metadata in SQLite by default, at `<storage>/.metadata/metadata.sqlite`. The `metadata` block takes the database as a URL, such as `url: sqlite:./metadata.sqlite`, and no longer accepts `backend` or `path`. A registry with an `s3:` block and no metadata URL keeps package documents in the bucket. pnpr does not import the package documents of an existing filesystem store, so republish its packages after upgrading.
