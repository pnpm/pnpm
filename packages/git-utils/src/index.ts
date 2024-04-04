import execa from 'execa'

// git checks logic is from https://github.com/sindresorhus/np/blob/master/source/git-tasks.js

export async function isGitRepo () {
  try {
    await execa('git', ['rev-parse', '--git-dir'])
  } catch {
    return false
  }
  return true
}

export async function getCurrentBranch () {
  try {
    const { stdout } = await execa('git', ['symbolic-ref', '--short', 'HEAD'])
    return stdout
  } catch {
    // Command will fail with code 1 if the HEAD is detached.
    return null
  }
}

export async function isWorkingTreeClean () {
  try {
    const { stdout: status } = await execa('git', ['status', '--porcelain'])
    if (status !== '') {
      return false
    }
    return true
  } catch {
    return false
  }
}

export async function isRemoteHistoryClean () {
  let history
  try { // Gracefully handle no remote set up.
    const { stdout } = await execa('git', ['rev-list', '--count', '--left-only', '@{u}...HEAD'])
    history = stdout
  } catch {
    history = null
  }
  if (history && history !== '0') {
    return false
  }
  return true
}
