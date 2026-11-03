---
"@pnpm/pnpr": minor
---

pnpr stages release archives uploaded by integrity before the publish that references them: `PUT /-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` without an owner stores an archive for the uploading account, and `HEAD` on the same URL tells whether it is staged. A staged archive is never served, only its uploader can claim it, and it expires 24 hours after its last upload.
