import '@total-typescript/ts-reset'

import execa from 'execa'

// git checks logic is from https://github.com/sindresorhus/np/blob/master/source/git-tasks.js

export async function isGitRepo(): Promise<boolean> {
  try {
    await execa.default('git', ['rev-parse', '--git-dir'])
  } catch (_: unknown) {
    return false
  }

  return true
}

export async function getCurrentBranch(): Promise<string | null> {
  try {
    const { stdout } = await execa.default('git', ['symbolic-ref', '--short', 'HEAD'])

    return stdout
  } catch (_: unknown) {
    // Command will fail with code 1 if the HEAD is detached.
    return null
  }
}

export async function isWorkingTreeClean(): Promise<boolean> {
  try {
    const { stdout: status } = await execa.default('git', ['status', '--porcelain'])

    if (status !== '') {
      return false
    }

    return true
  } catch (_: unknown) {
    return false
  }
}

export async function isRemoteHistoryClean(): Promise<boolean> {
  let history
  try {
    // Gracefully handle no remote set up.
    const { stdout } = await execa.default('git', [
      'rev-list',
      '--count',
      '--left-only',
      '@{u}...HEAD',
    ])
    history = stdout
  } catch (_: unknown) {
    history = null
  }
  if (history && history !== '0') {
    return false
  }
  return true
}
