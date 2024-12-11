---
"@pnpm/lifecycle": major
"pnpm": major
---

Reduced the number of fields from `package.json` that are added as environment variables (`npm_package_` prefix) during script execution. Only the following fields are now included: `name`, `version`, `bin`, `engines`, and `config` [#8552](https://github.com/pnpm/pnpm/issues/8552).
