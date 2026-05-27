---
"@pnpm/config": patch
"@pnpm/network.auth-header": major
"pnpm": patch
---

Pin unscoped per-registry settings (`_authToken`, `_auth`, `username`/`_password`, `tokenHelper`, inline `cert`/`key`) to the registry declared in the same config source at load time, so a later layer overriding `registry=` (workspace `.npmrc`, `pnpm-workspace.yaml`, CLI `--registry`) cannot redirect a credential or client certificate authored for a different host. A deprecation warning is emitted whenever an unscoped per-registry setting is encountered, naming the source and the URL it was pinned to. Reported by JUNYI LIU.
