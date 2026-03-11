import { execSync } from 'child_process'
import path from 'path'

const pr = process.argv[2]
if (!pr) {
  console.error('Usage: pnpm worktree:pr <pr-number>')
  process.exit(1)
}

const repoRoot = execSync('git rev-parse --show-toplevel', { encoding: 'utf8' }).trim()
const localBranch = `pr-${pr}`
const worktreePath = path.join(path.dirname(repoRoot), localBranch)

// Git output goes to stderr so stdout carries only the path (enables: cd $(pnpm worktree:pr <number>))
const gitStdio = ['inherit', process.stderr, process.stderr] as const

// Fetch the PR head (works for both same-repo branches and forks)
execSync(`git fetch origin "pull/${pr}/head:${localBranch}"`, { stdio: gitStdio, cwd: repoRoot })

// Create the worktree for the fetched branch
execSync(`git worktree add "${worktreePath}" "${localBranch}"`, { stdio: gitStdio, cwd: repoRoot })

// Print path to stdout — allows: cd $(pnpm worktree:pr <number>)
process.stdout.write(worktreePath + '\n')
