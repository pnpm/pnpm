---
"@pnpm/plugin-commands-store-inspecting": minor
"@pnpm/package-requester": major
"@pnpm/plugin-commands-rebuild": major
"@pnpm/plugin-commands-store": major
"@pnpm/license-scanner": major
"@pnpm/assert-project": major
"@pnpm/assert-store": major
"@pnpm/mount-modules": minor
"@pnpm/headless": major
"@pnpm/package-store": major
"@pnpm/core": major
"@pnpm/store.cafs": major
"pnpm": major
---

Some registries allow identical content to be published under different package names or versions. To accommodate this, index files in the store are now stored using both the content hash and package identifier.

This approach ensures that we can:
1. Validate that the integrity in the lockfile corresponds to the correct package,
   which might not be the case after a poorly resolved Git conflict.
2. Allow the same content to be referenced by different packages or different versions of the same package.

Related PR: [#8510](https://github.com/pnpm/pnpm/pull/8510)
Related issue: [#8204](https://github.com/pnpm/pnpm/issues/8204)
