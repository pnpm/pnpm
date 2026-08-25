# @pnpm.e2e/reads-consumer-manifest

A package whose `postinstall` reads the `package.json` of the directory above
the `node_modules` that holds it — the convention every git-hook installer uses
to find the project that installed it.

It exists to pin that this read does not crash an install, whatever pnpm's
virtual store layout is. See https://github.com/pnpm/pnpm/issues/13318.
