import PnpmError from '@pnpm/error'
import execa = require('execa')

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
  } catch (_) {}
  if (history && history !== '0') {
    return false
  }
  return true
}
