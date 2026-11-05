---
"pacquet": minor
---

When `supportedArchitectures` names exactly one platform and not `current`, pnpm links that platform's variant of a package published with platform variants, together with that variant's dependencies. This holds for the list form and for the `os`, `cpu`, and `libc` form. A variant whose floor or ABI the listed platform does not state is not linked for it: pnpm links another variant that fits, or the version's own archive. Changing the setting, or the platform of the host, such as a `node_modules` mounted into a musl container or a Node.js with another ABI, relinks the package on the next install, even from a frozen lockfile.
