// Enforces the one-worktree-one-branch rule for agents: a linked
// worktree keeps the branch it was bound to for its whole life, so two
// workers never end up sharing (or repurposing) the same directory.
//
// The binding is recorded in `<git-dir>/agent-bound-branch`. A branch
// checkout observed in a worktree with no recorded binding binds it to
// the branch the worktree was on *before* the switch (recovered from
// the checkout history), so even the first switch is checked. Branch
// checkouts away from the bound branch are rejected for agent sessions
// (`CLAUDECODE` set); returning to the bound branch is always allowed.
// Humans may rebind freely — doing so moves the binding. Rationale
// lives in the error message below and in AGENTS.md ("Worktrees").
//
// Invoked two ways:
//   post-checkout: reject-worktree-rebind.mjs <old> <new> <flag>
//   other hooks:   reject-worktree-rebind.mjs --bind-only
// `--bind-only` records the binding if missing and never rejects, so a
// worktree in which this hook has never observed a checkout still
// becomes bound through its normal use (commits).

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8' }).trim()

// Single-quote a value for a copy/paste-safe POSIX shell command.
const shellQuote = (value) => `'${value.replaceAll("'", String.raw`'\''`)}'`

const bindOnly = process.argv[2] === '--bind-only'
// post-checkout's third argument is 1 for a branch checkout, 0 for a
// file checkout; only branch checkouts can rebind a worktree.
if (!bindOnly && process.argv[4] !== '1') {
  process.exit(0)
}

// `--git-dir` / `--git-common-dir` may print worktree-relative paths;
// hooks run from the worktree's top level, so resolving against the
// working directory absolutizes both.
const gitDir = resolve(git('rev-parse', '--git-dir'))
const gitCommonDir = resolve(git('rev-parse', '--git-common-dir'))
// The main checkout is free to switch branches; only linked worktrees
// are single-branch workspaces.
if (gitDir === gitCommonDir) {
  process.exit(0)
}

let branch
try {
  branch = git('symbolic-ref', '--quiet', '--short', 'HEAD')
} catch (error) {
  // `--quiet` exits 1 on a detached HEAD (rebase, bisect, CI
  // checkout), which is not a rebind. Anything else is an unexpected
  // failure the guard must not silently fail open on.
  if (error?.status === 1) {
    process.exit(0)
  }
  console.error(`reject-worktree-rebind: cannot read HEAD: ${error?.message ?? error}`)
  process.exit(1)
}

const markerPath = join(gitDir, 'agent-bound-branch')
let bound = existsSync(markerPath) ? readFileSync(markerPath, 'utf8').trim() : null
if (bound === null) {
  bound = initialBinding(branch)
  writeFileSync(markerPath, `${bound}\n`)
}

if (bound === branch || bindOnly) {
  process.exit(0)
}

const isAgent = Boolean(process.env.CLAUDECODE)
const overridden = process.env.PNPM_ALLOW_WORKTREE_REBIND === '1'
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
    `  git switch ${shellQuote(bound)}`,
    `  git worktree add ${shellQuote(`../${branch.replaceAll('/', '-')}`)} ${shellQuote(branch)}`,
    '',
    'A human who really wants to rebind this worktree can re-run the checkout with',
    'PNPM_ALLOW_WORKTREE_REBIND=1.',
  ].join('\n')
)
process.exit(1)

// The branch a first-observed checkout binds the worktree to: the
// branch the worktree was on before the switch, so the switch itself
// still gets checked. The worktree's own creation (old ref is the null
// SHA) and a checkout history that names no prior branch bind to the
// branch just checked out.
function initialBinding (currentBranch) {
  const oldRef = bindOnly ? null : process.argv[2]
  if (!oldRef || /^0+$/.test(oldRef)) {
    return currentBranch
  }
  try {
    const previous = git('rev-parse', '--symbolic-full-name', '@{-1}')
    if (previous.startsWith('refs/heads/')) {
      return previous.slice('refs/heads/'.length)
    }
  } catch {
    // No prior checkout recorded; fall through.
  }
  return currentBranch
}
