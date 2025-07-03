---
"@pnpm/lifecycle": patch
---

When running the `pnpm run` command and using a regular expression to match multiple script names, an error should be thrown if the script name matches the current script name.
