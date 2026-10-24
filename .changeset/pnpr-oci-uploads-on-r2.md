---
"@pnpm/pnpr": patch
---

Fixed OCI image pushes to pnpr when its S3 storage is Cloudflare R2. Every blob upload failed with a `blob_upload_conflict` error.
