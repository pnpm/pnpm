---
"@pnpm/cli-utils": major
"pnpm": major
---

pnpm will now check the `package.json` file for a `packageManager` field. If this field is present and specifies a different package manager or a different version of pnpm than the one you're currently using, pnpm will not proceed. This ensures that you're always using the correct package manager and version that the project requires.
