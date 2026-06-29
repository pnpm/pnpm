// Points git at this directory so the commit-msg checks run, replacing what
// husky used to do. The root package runs it from its "prepare" script, so a
// plain `pnpm install` wires the hooks up.
//
// A relative hooksPath is resolved against each working tree, so this works for
// the bare-repo + worktrees layout described in CONTRIBUTING.md too.

import { execFileSync } from 'node:child_process'

// Outside a git repo (e.g. when the package is unpacked from a tarball during
// publishing) there is nothing to wire up — succeed silently, like husky did.
try {
  execFileSync('git', ['rev-parse', '--git-dir'], { stdio: 'ignore' })
} catch {
  process.exit(0)
}

execFileSync('git', ['config', 'core.hooksPath', '.githooks'])
