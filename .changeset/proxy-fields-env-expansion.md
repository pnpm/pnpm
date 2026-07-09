---
"@pnpm/config.reader": patch
"pnpm": patch
---

Do not expand environment variable placeholders in the `httpProxy`, `httpsProxy`, and `noProxy` settings when they are read from a project's `pnpm-workspace.yaml`. Previously these proxy fields expanded `${...}` placeholders from project-level config, so a malicious repository could exfiltrate secrets present at install time (such as CI tokens) by routing requests through an attacker-controlled proxy whose URL embedded the secret. These fields now receive the same protection already applied to `registry`, `namedRegistries`, and `pnprServer`.
