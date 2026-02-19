---
"@pnpm/plugin-commands-config": major
"pnpm": major
---

Stop falling back to the npm CLI. Commands that were previously passed through to npm (`access`, `adduser`, `bugs`, `deprecate`, `dist-tag`, `docs`, `edit`, `find`, `home`, `info`, `issues`, `login`, `logout`, `owner`, `ping`, `prefix`, `profile`, `pkg`, `repo`, `search`, `set-script`, `show`, `star`, `stars`, `team`, `token`, `unpublish`, `unstar`, `version`, `view`, `whoami`, `xmas`) now throw a "not implemented" error. The `pnpm config` command no longer delegates auth-related settings to npm and instead writes them directly to the rc file.
