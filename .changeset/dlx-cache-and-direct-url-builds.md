---
"@pnpm/config.reader": patch
"@pnpm/exec.commands": patch
"@pnpm/global.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

- `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
- Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
- Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
- Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.
