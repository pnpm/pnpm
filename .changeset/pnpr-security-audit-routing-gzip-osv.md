---
"@pnpm/pnpr": patch
---

Router registries now partition npm security audit requests by package route, keeping hosted packages local and sending each upstream only the packages routed to it. The audit endpoints now accept `Content-Encoding: gzip` requests such as those sent by `npm audit`. `POST /-/npm/v1/security/audits` and `POST /-/npm/v1/security/audits/quick` now also evaluate advisories from the local OSV database when `--osv` is enabled.
