---
"@pnpm/pnpr": minor
---

The pnpr resolver refuses a request with a field it does not know, with the error code `ERR_PNPR_UNSUPPORTED_REQUEST_FIELD`. A request's `acceptedLockfileFormats` now names formats such as `pnpm-lock/9.0`. Every resolve `done` frame names the format of its lockfile in `lockfileFormat`. A streamed resolve sends a `progress` frame every 15 seconds.
