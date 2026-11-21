---
"@pnpm/pnpr": patch
---

When the version tagged `latest` is removed, pnpr now moves the tag to the highest remaining stable release if one exists. Moving `latest` also updates the package README.

pnpr rejects npm publish documents with no versions. Fetching package metadata with `?write=true` returns the current revision. Deprecation updates check that revision on every request, reject non-string messages, and clear the annotation for an empty message.
