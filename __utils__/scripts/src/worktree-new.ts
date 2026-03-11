import { execSync } from 'child_process'
import path from 'path'

const branch = process.argv[2]
if (!branch) {
  console.error('Usage: pnpm worktree:new <branch-name>')
  process.exit(1)
}

const repoRoot = execSync('git rev-parse --show-toplevel', { encoding: 'utf8' }).trim()
const safeBranch = branch.replace(/\//g, '-')
const worktreePath = path.join(path.dirname(repoRoot), safeBranch)

// Git output goes to stderr so stdout carries only the path (enables: cd $(pnpm worktree:new <branch>))
const gitStdio = ['inherit', process.stderr, process.stderr] as const

try {
  // Checkout existing branch
  execSync(`git worktree add "${worktreePath}" "${branch}"`, { stdio: gitStdio, cwd: repoRoot })
} catch {
  // Branch doesn't exist yet — create it from main
  execSync(`git worktree add -b "${branch}" "${worktreePath}" main`, { stdio: gitStdio, cwd: repoRoot })
}

// Print path to stdout — allows: cd $(pnpm worktree:new <branch>)
process.stdout.write(worktreePath + '\n')
