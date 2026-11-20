// Points git at this directory so the hooks in it run. The root package runs
// this from its "prepare" script, so a plain `pnpm install` wires the hooks up.
//
// A relative hooksPath is resolved against each working tree, so this works for
// the bare-repo + worktrees layout described in CONTRIBUTING.md too.

import { execFileSync } from 'node:child_process'
import process from 'node:process'

const HOOKS_PATH = '.config/githooks'

// The hooks are a developer safety net; CI has its own gates, and a workflow
// that commits or pushes must not pay for the pre-push compile and lint.
if (process.env.CI) {
  process.exit(0)
}

// Outside a git repo (e.g. when the package is unpacked from a tarball) there
// is nothing to wire up.
try {
  execFileSync('git', ['rev-parse', '--git-dir'], { stdio: 'ignore' })
} catch {
  process.exit(0)
}

execFileSync('git', ['config', 'core.hooksPath', HOOKS_PATH])
