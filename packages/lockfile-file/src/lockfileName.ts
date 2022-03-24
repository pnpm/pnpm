import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getCurrentBranchName } from './gitChecks'

export interface GetWantedLockfileNameOptions {
  useGitBranchLockfile?: boolean
}

export function getWantedLockfileName (opts: GetWantedLockfileNameOptions = { useGitBranchLockfile: false }) {
  if (opts.useGitBranchLockfile) {
    const currentBranchName = getCurrentBranchName()
    if (currentBranchName) {
      return WANTED_LOCKFILE.replace('.yaml', `.${stringifyBranchName(currentBranchName)}.yaml`)
    }
  }
  return WANTED_LOCKFILE
}

// branch name may contains slashes, which is not allowed in filenames
function stringifyBranchName (branchName: string = '') {
  return branchName.replace(/[^a-zA-Z0-9-_.]/g, '!')
}