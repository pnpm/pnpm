---
"@pnpm/pnpr": patch
---

When the version tagged `latest` is removed, pnpr now restores the most recent former `latest` that is still published. If none remains, pnpr prefers the highest remaining stable release. Moving `latest` also updates the package README.

pnpr rejects npm publish documents with no versions. Fetching package metadata with `?write=true` returns the current revision. Deprecation updates check that revision on every request, reject non-string messages, and clear the annotation for an empty message.
