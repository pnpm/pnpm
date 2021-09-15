import git from 'graceful-git'

// git checks logic is from https://github.com/sindresorhus/np/blob/master/source/git-tasks.js

export async function isGitRepo () {
  try {
    await git.noRetry(['rev-parse', '--git-dir'])
  } catch (_) {
    return false
  }
  return true
}

export async function getCurrentBranch (): Promise<string> {
  const { stdout } = await git.noRetry(['symbolic-ref', '--short', 'HEAD'])
  return stdout
}

export async function isWorkingTreeClean () {
  try {
    const { stdout: status } = await git.noRetry(['status', '--porcelain'])
    if (status !== '') {
      return false
    }
    return true
  } catch (_) {
    return false
  }
}

export async function isRemoteHistoryClean () {
  let history
  try { // Gracefully handle no remote set up.
    const { stdout } = await git.noRetry(['rev-list', '--count', '--left-only', '@{u}...HEAD'])
    history = stdout
  } catch (_) {
    history = null
  }
  if (history && history !== '0') {
    return false
  }
  return true
}
