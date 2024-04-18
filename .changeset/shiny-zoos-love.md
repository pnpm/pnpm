---
"@pnpm/lifecycle": patch
pnpm: patch
---

If the `script-shell` option is configured to a `.bat`/`.cmd` file on Windows, pnpm will now error with `ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS`. Newer [versions of Node.js released in April 2024](https://nodejs.org/en/blog/vulnerability/april-2024-security-releases-2) do not support executing these files directly without behavior differences. If the `script-shell` option is necessary for your use-case, please set a `.exe` file instead.
