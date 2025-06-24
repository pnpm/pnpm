---
"@pnpm/core": patch
"pnpm": patch
---

Restore hoisting of optional peer dependencies when installing with an outdated lockfile.
Regression introduced in [v10.12.2] by [#9648]; resolves [#9685].

[v10.12.2]: https://github.com/pnpm/pnpm/releases/tag/v10.12.2
[#9648]: https://github.com/pnpm/pnpm/pull/9648
[#9685]: https://github.com/pnpm/pnpm/issues/9685
