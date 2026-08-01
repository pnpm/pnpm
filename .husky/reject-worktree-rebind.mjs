// Enforces the one-worktree-one-branch rule for agents: a linked
// worktree keeps the branch it was bound to for its whole life, so two
// workers never end up sharing (or repurposing) the same directory.
//
// The binding is recorded in `<git-dir>/agent-bound-branch` the first
// time any wired hook runs in the worktree. Later branch checkouts in
// that worktree are rejected for agent sessions (`CLAUDECODE` set)
// unless they return to the bound branch. Humans may rebind freely —
// doing so moves the binding. Rationale lives in the error message
// below and in AGENTS.md ("Worktrees").
//
// Invoked two ways:
//   post-checkout: reject-worktree-rebind.mjs <old> <new> <flag>
//   other hooks:   reject-worktree-rebind.mjs --bind-only
// `--bind-only` records the binding if missing and never rejects, so a
// worktree that predates this guard becomes bound by its normal use
// (commits, pushes) before any rebind can slip through.

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8' }).trim()

const bindOnly = process.argv[2] === '--bind-only'
// post-checkout's third argument is 1 for a branch checkout, 0 for a
// file checkout; only branch checkouts can rebind a worktree.
if (!bindOnly && process.argv[4] !== '1') {
  process.exit(0)
}

const gitDir = git('rev-parse', '--absolute-git-dir')
const gitCommonDir = git('rev-parse', '--path-format=absolute', '--git-common-dir')
// The main checkout is free to switch branches; only linked worktrees
// are single-branch workspaces.
if (gitDir === gitCommonDir) {
  process.exit(0)
}

let branch
try {
  branch = git('symbolic-ref', '--quiet', '--short', 'HEAD')
} catch {
  // Detached HEAD (rebase, bisect, CI checkout) is not a rebind.
  process.exit(0)
}

const markerPath = join(gitDir, 'agent-bound-branch')
const bound = existsSync(markerPath) ? readFileSync(markerPath, 'utf8').trim() : null

if (bound === null || bound === branch) {
  if (bound === null) {
    writeFileSync(markerPath, `${branch}\n`)
  }
  process.exit(0)
}

if (bindOnly) {
  process.exit(0)
}

const isAgent = Boolean(process.env.CLAUDECODE)
const overridden = Boolean(process.env.PNPM_ALLOW_WORKTREE_REBIND)
if (!isAgent || overridden) {
  writeFileSync(markerPath, `${branch}\n`)
  process.exit(0)
}

console.error(
  [
    `error: This worktree is bound to the branch \`${bound}\`, but it was just switched to \`${branch}\`.`,
    '',
    'One worktree works on one branch for its whole life, so concurrent workers',
    'never collide in a shared directory. Undo the switch and start the new branch',
    'in a fresh worktree instead:',
    '',
    `  git switch ${bound}`,
    `  git worktree add ../${branch.replaceAll('/', '-')} ${branch}`,
    '',
    'A human who really wants to rebind this worktree can re-run the checkout with',
    'PNPM_ALLOW_WORKTREE_REBIND=1.',
  ].join('\n')
)
process.exit(1)
