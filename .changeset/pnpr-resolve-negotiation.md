---
"@pnpm/pnpr": minor
---

The pnpr resolver refuses a request with a field it does not know, with the error code `ERR_PNPR_UNSUPPORTED_REQUEST_FIELD`. A request's `acceptedLockfileFormats` now names formats such as `pnpm-lock/9.0`, and every resolve `done` frame names the format of its lockfile in `lockfileFormat`. Long streamed resolves send `progress` frames, so proxies with an idle timeout keep the connection open.
