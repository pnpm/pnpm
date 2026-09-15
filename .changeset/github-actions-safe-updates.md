---
"@pnpm/deps.github-actions": patch
"pnpm": patch
"pacquet": patch
---

GitHub Actions updates now stop if an action reference changes while its versions are being resolved. Unrelated workflow edits are preserved.

GitHub Actions homepage links no longer expose server credentials. GitHub server URLs now require HTTPS, with HTTP allowed only for loopback hosts.
