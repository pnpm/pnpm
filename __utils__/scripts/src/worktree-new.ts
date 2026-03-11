import { execSync } from 'child_process'
import fs from 'fs'
import path from 'path'

const arg = process.argv[2]
if (!arg) {
  console.error('Usage: pnpm worktree:new <branch-name|pr-number>')
  process.exit(1)
}

const repoRoot = execSync('git rev-parse --show-toplevel', { encoding: 'utf8' }).trim()

// Git output goes to stderr so stdout carries only the path (enables: cd $(pnpm worktree:new <arg>))
const gitStdio = ['inherit', process.stderr, process.stderr] as const

let localBranch: string
let worktreePath: string

if (/^\d+$/.test(arg)) {
  // PR number — fetch via GitHub's pull request ref (works for forks too)
  localBranch = `pr-${arg}`
  worktreePath = path.join(path.dirname(repoRoot), localBranch)
  execSync(`git fetch origin "pull/${arg}/head:${localBranch}"`, { stdio: gitStdio, cwd: repoRoot })
  execSync(`git worktree add "${worktreePath}" "${localBranch}"`, { stdio: gitStdio, cwd: repoRoot })
} else {
  // Branch name — slashes replaced with dashes for the directory name
  localBranch = arg
  worktreePath = path.join(path.dirname(repoRoot), arg.replace(/\//g, '-'))
  try {
    // Checkout existing branch
    execSync(`git worktree add "${worktreePath}" "${localBranch}"`, { stdio: gitStdio, cwd: repoRoot })
  } catch {
    // Branch doesn't exist yet — create it from main
    execSync(`git worktree add -b "${localBranch}" "${worktreePath}" main`, { stdio: gitStdio, cwd: repoRoot })
  }
}

// Symlink .claude from the new worktree to the main worktree so Claude Code
// settings and approved commands are shared across all worktrees.
const mainClaudeDir = fs.realpathSync(path.join(repoRoot, '.claude'))
const newClaudeDir = path.join(worktreePath, '.claude')
if (fs.existsSync(mainClaudeDir) && !fs.existsSync(newClaudeDir)) {
  fs.symlinkSync(mainClaudeDir, newClaudeDir)
}

// Print path to stdout — allows: cd $(pnpm worktree:new <arg>)
process.stdout.write(worktreePath + '\n')
