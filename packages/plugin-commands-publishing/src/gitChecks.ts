import PnpmError from '@pnpm/error'
import execa = require('execa')

// git checks logic is from https://github.com/sindresorhus/np/blob/master/source/git-tasks.js

export async function gitChecks () {
  if (await getCurrentBranch() !== 'master') {
    throw new PnpmError('GIT_CHECK_FAILED', "Branch is not on 'master'.")
  }
  if (!(await isWorkingTreeClean())) {
    throw new PnpmError('GIT_CHECK_FAILED', 'Unclean working tree. Commit or stash changes first.')
  }
  if (!(await isRemoteHistoryClean())) {
    throw new PnpmError('GIT_CHECK_FAILED', 'Remote history differs. Please pull changes.')
  }
}

export async function isGitRepo () {
  try {
    await execa('git', ['rev-parse', '--git-dir'])
  } catch (_) {
    return false
  }
  return true
}

async function getCurrentBranch () {
  const { stdout } = await execa('git', ['symbolic-ref', '--short', 'HEAD'])
  return stdout
}

async function isWorkingTreeClean () {
  try {
    const { stdout: status } = await execa('git', ['status', '--porcelain'])
    if (status !== '') {
      return false
    }
    return true
  } catch (_) {
    return false
  }
}

async function isRemoteHistoryClean () {
  let history
  try { // Gracefully handle no remote set up.
    const { stdout } = await execa('git', ['rev-list', '--count', '--left-only', '@{u}...HEAD'])
    history = stdout
  } catch (_) {
    history = null
  }
  if (history && history !== '0') {
    return false
  }
  return true
}
