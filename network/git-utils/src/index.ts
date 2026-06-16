import fs from 'node:fs'
import path from 'node:path'

import { safeExeca as execa } from 'execa'

// git checks logic is from https://github.com/sindresorhus/np/blob/master/source/git-tasks.js

export interface GitCwdOptions {
  cwd?: string
}

export async function isGitRepo (opts: GitCwdOptions = {}): Promise<boolean> {
  try {
    await execa('git', ['rev-parse', '--git-dir'], { cwd: opts.cwd })
  } catch {
    return false
  }
  return true
}

export async function getCurrentBranch (opts: GitCwdOptions = {}): Promise<string | null> {
  const branch = readBranchFromHeadFile(opts.cwd)
  if (branch !== undefined) return branch
  try {
    const { stdout } = await execa('git', ['symbolic-ref', '--short', 'HEAD'], { cwd: opts.cwd })
    return stdout as string
  } catch {
    // Command will fail with code 1 if the HEAD is detached.
    return null
  }
}

export async function isWorkingTreeClean (opts: GitCwdOptions = {}): Promise<boolean> {
  try {
    const { stdout: status } = await execa('git', ['status', '--porcelain'], { cwd: opts.cwd })
    if (status !== '') {
      return false
    }
    return true
  } catch {
    return false
  }
}

export async function isRemoteHistoryClean (opts: GitCwdOptions = {}): Promise<boolean> {
  let history
  try { // Gracefully handle no remote set up.
    const { stdout } = await execa('git', ['rev-list', '--count', '--left-only', '@{u}...HEAD'], { cwd: opts.cwd })
    history = stdout
  } catch {
    history = null
  }
  if (history && history !== '0') {
    return false
  }
  return true
}

/**
 * Reads the current branch name from `.git/HEAD` without spawning a git subprocess.
 *
 * Returns:
 * - `string` — the branch name extracted from `ref: refs/heads/<name>`
 * - `null` — HEAD is detached (a raw commit SHA, not a symbolic ref)
 * - `undefined` — `.git/HEAD` could not be read (not a git repo, worktree
 *   layout not recognized, permissions error, etc.); caller should fall
 *   back to `git symbolic-ref`.
 */
function readBranchFromHeadFile (cwd?: string): string | null | undefined {
  const baseDir = cwd ?? process.cwd()
  const dotGitPath = path.join(baseDir, '.git')
  let gitDir: string
  try {
    const stat = fs.statSync(dotGitPath)
    if (stat.isDirectory()) {
      gitDir = dotGitPath
    } else if (stat.isFile()) {
      // `.git` is a file — worktree or submodule. It contains `gitdir: <path>`.
      const content = fs.readFileSync(dotGitPath, 'utf8').trim()
      const match = content.match(/^gitdir:\s*(.+)/)
      if (!match) return undefined
      gitDir = path.isAbsolute(match[1]!) ? match[1]! : path.resolve(baseDir, match[1]!)
    } else {
      // `.git` is neither a directory nor a regular file (e.g. a FIFO or
      // device); don't read it. Fall back to `git symbolic-ref`.
      return undefined
    }
  } catch {
    return undefined
  }
  try {
    const head = fs.readFileSync(path.join(gitDir, 'HEAD'), 'utf8').trim()
    const match = head.match(/^ref:\s*refs\/heads\/(.+)/)
    if (match) return match[1]!
    return null
  } catch {
    return undefined
  }
}
