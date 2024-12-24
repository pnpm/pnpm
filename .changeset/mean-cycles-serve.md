---
"@pnpm/default-reporter": minor
"@pnpm/build-modules": minor
"pnpm": patch
---

Improve how packages with blocked lifecycle scripts are reported during installation. Always print the list of ignored scripts at the end of the output. Include a hint about how to allow the execution of those packages.
