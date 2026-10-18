---
"pacquet": minor
---

Registry revisions now always pin the hash of the uncompressed tar, the original included. pnpm records an original selected through the revision protocol, for example with a `+r0` specifier, as `revision: 0` in the lockfile and fetches it from the registry's digest route. pnpm checks a revision's tar whether the registry sends it as a tar, gzip, or zstd. Revision metadata must list revision 0 and mark the selected revision. pnpm rejects incomplete revision metadata with `ERR_PNPM_MALFORMED_METADATA`.
