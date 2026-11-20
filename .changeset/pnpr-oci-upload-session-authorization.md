---
"@pnpm/pnpr": patch
---

OCI blob upload continuation requests on `/v2/<name>/blobs/uploads/<id>` now check current repository publish permissions and require the caller who started the upload session.
