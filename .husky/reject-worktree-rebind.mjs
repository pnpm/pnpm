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

// Child stderr is piped (not inherited) so an expected failure — a
// detached HEAD, an unresolvable `@{-1}` — doesn't splash `fatal:`
// noise into every checkout; the messages this script prints carry
// the signal instead.
const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()

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

const isAgent = Boolean(process.env.CLAUDECODE)
const overridden = process.env.PNPM_ALLOW_WORKTREE_REBIND === '1'

const markerPath = join(gitDir, 'agent-bound-branch')
let bound = existsSync(markerPath) ? readFileSync(markerPath, 'utf8').trim() : null
if (bound === null) {
  bound = initialBinding(branch)
  if (bound === null) {
    // A first-observed switch whose origin the checkout history cannot
    // name would otherwise bind to its own destination — the exact
    // rebind this guard exists to reject. Fail closed for agents and
    // leave the worktree unbound.
    console.error(
      [
        `error: This worktree has no recorded branch binding, and the checkout history does not name the branch this switch to \`${branch}\` came from.`,
        '',
        'One worktree works on one branch for its whole life. Switch back to the',
        "branch this worktree was on. If it is legitimately this worktree's own",
        'branch, record the binding with:',
        '',
        '  node .husky/reject-worktree-rebind.mjs --bind-only',
        '',
        'A human can instead re-run the checkout with PNPM_ALLOW_WORKTREE_REBIND=1.',
      ].join('\n')
    )
    process.exit(1)
  }
  writeFileSync(markerPath, `${bound}\n`)
}

if (bound === branch || bindOnly) {
  process.exit(0)
}

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

// The branch a first-observed checkout binds the worktree to: the most
// recent branch named as a checkout source in the worktree's HEAD
// reflog, so the switch itself still gets checked. Reflog subjects are
// read directly — `@{-N}` cannot name a since-deleted branch — and
// detached sources (a rebase or bisect in progress at the time) are
// skipped in favor of the branch they detached from. A same-branch
// re-checkout binds to itself through its own reflog entry; comparing
// the hook's old/new SHAs cannot stand in for that, because `switch
// -c` from the bound branch also moves nothing. The worktree's own
// creation (old ref is the null SHA) and a `--bind-only` call bind to
// the branch just checked out. `null` means a checkout happened whose
// history cannot name any source branch (reflog disabled, expired, or
// detached throughout), so agent sessions must not trust the
// destination.
function initialBinding (currentBranch) {
  const oldRef = bindOnly ? null : process.argv[2]
  if (!oldRef || /^0+$/.test(oldRef)) {
    return currentBranch
  }
  let subjects
  try {
    subjects = git('log', '-g', '--format=%gs', 'HEAD')
  } catch {
    subjects = ''
  }
  // The newest entry is the checkout that fired this hook; its source
  // is where the worktree stood before.
  for (const line of subjects.split('\n')) {
    const source = /^checkout: moving from (\S+) to /.exec(line)?.[1]
    if (!source || /^[0-9a-f]{40}$/.test(source)) {
      continue
    }
    return source
  }
  return isAgent && !overridden ? null : currentBranch
}
