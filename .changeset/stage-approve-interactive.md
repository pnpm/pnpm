---
"@pnpm/releasing.commands": minor
"@pnpm/network.web-auth": minor
"@pnpm/text.sanitize": minor
"@pnpm/installing.commands": patch
"pacquet": minor
"pnpm": minor
---

`pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Before approving the batch, pnpm downloads every selected tarball and reads its published manifest. The selected packages are approved in dependency order, and a package whose selected dependency could not be approved is skipped. A batch containing two stages for the same package version is rejected before any stage is approved.
