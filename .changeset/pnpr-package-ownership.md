---
"@pnpm/pnpr": patch
---

Record package maintainers on publish, support the `$owners` access token in package access rules, and handle `npm owner` maintainer mutations. Record unpublished packages and versions in tombstones to protect against package squatting and republishing of removed versions [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
