---
"pacquet": minor
---

Added `pnpm publish --publish-wait-timeout <milliseconds>` to wait for published versions and their tarballs to become available from the registry. Set `publishWaitTimeout` in `pnpm-workspace.yaml` to configure a default. A value of `0` disables the check.

Recursive publishing confirms availability before publishing dependent packages. If confirmation times out, the command fails.

When `pnpm publish -r --report-summary` fails after some uploads were accepted, the summary file now lists those packages.
