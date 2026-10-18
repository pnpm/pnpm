---
"pacquet": minor
---

Registry revisions now always pin the hash of the uncompressed tar, the original included. An original selected through the revision protocol, for example with a `+r0` specifier, is recorded in the lockfile as `revision: 0` and fetched from the registry's digest route. pnpm checks a revision's tar whether the registry sends it as a tar, gzip, or zstd. Revision metadata must list revision 0 and mark the selected revision. pnpm rejects incomplete revision metadata with `ERR_PNPM_MALFORMED_METADATA`.
