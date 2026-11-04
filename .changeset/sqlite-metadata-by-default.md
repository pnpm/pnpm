---
"@pnpm/pnpr": minor
---

pnpr now keeps hosted package metadata in SQLite by default, at `<storage>/.metadata/metadata.sqlite`. The `metadata` block names the database by URL, such as `url: sqlite:./metadata.sqlite`. The `metadata` block no longer accepts `backend: sqlite` or `path`. A registry with an `s3:` block and no metadata URL keeps package documents in the bucket. pnpr does not import the package documents of an existing filesystem store. Republish its packages after upgrading.
