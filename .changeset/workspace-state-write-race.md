---
"@pnpm/workspace.state": patch
"pnpm": patch
---

On Windows, pnpm now retries writing the workspace state file while another process, such as an antivirus scanner, briefly holds it open [#14550](https://github.com/pnpm/pnpm/issues/14550).
