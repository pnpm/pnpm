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
