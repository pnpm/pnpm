---
"@pnpm/pnpr": patch
---

Approving a staged version now publishes its release notes. When different authenticated accounts stage and approve the version, pnpr records the approver and reports `stagedPublish` trust. Downloading a staged version with provenance returns the package tarball.
