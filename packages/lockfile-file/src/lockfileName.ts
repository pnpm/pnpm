import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getCurrentBranch } from '@pnpm/git-utils'

export interface GetWantedLockfileNameOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export async function getWantedLockfileName (opts: GetWantedLockfileNameOptions = { useGitBranchLockfile: false, mergeGitBranchLockfiles: false }) {
  if (opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles) {
    const currentBranchName = await getCurrentBranch()
    if (currentBranchName) {
      return WANTED_LOCKFILE.replace('.yaml', `.${stringifyBranchName(currentBranchName)}.yaml`)
    }
  }
  return WANTED_LOCKFILE
}

/**
 * 1. Git branch name may contains slashes, which is not allowed in filenames
 * 2. Filesystem may be case-insensitive, so we need to convert branch name to lowercase
 */
function stringifyBranchName (branchName: string = '') {
  return branchName.replace(/[^a-zA-Z0-9-_.]/g, '!').toLowerCase()
}