---
"@pnpm/pnpr": patch
---

Self-registration can no longer claim the username of a configured admin, an OIDC binding, a workload binding, or a SCIM-provisioned user. The new `pnpr add-user` command creates accounts under reserved usernames [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
