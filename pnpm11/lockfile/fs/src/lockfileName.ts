import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getBranchesContainingHead, getCurrentBranch } from '@pnpm/network.git-utils'

export interface GetWantedLockfileNameOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  cwd?: string
}

export async function getWantedLockfileName (opts: GetWantedLockfileNameOptions = {}): Promise<string> {
  if (opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles) {
    const currentBranchName = await getCurrentBranch({ cwd: opts.cwd })
    if (currentBranchName) {
      return WANTED_LOCKFILE.replace('.yaml', `.${stringifyBranchName(currentBranchName)}.yaml`)
    }
  }
  return WANTED_LOCKFILE
}

/**
 * The branch lockfile files the wanted-lockfile read tries before
 * `pnpm-lock.yaml` itself, in order.
 *
 * On a branch that is the branch's own lockfile. On a detached HEAD no
 * branch is checked out, but the checked-out commit still belongs to the
 * branches whose history includes it, so their lockfiles are what can
 * satisfy the checkout — the read tries each of them before the shared
 * lockfile. `mergeGitBranchLockfiles` reads the shared lockfile and folds
 * every branch lockfile in, so it names none of them here.
 */
export async function getWantedLockfileNames (opts: GetWantedLockfileNameOptions = {}): Promise<string[]> {
  if (!opts.useGitBranchLockfile || opts.mergeGitBranchLockfiles) {
    return []
  }
  const currentBranchName = await getCurrentBranch({ cwd: opts.cwd })
  if (currentBranchName) {
    return [branchLockfileName(currentBranchName)]
  }
  const branchesContainingHead = await getBranchesContainingHead({ cwd: opts.cwd })
  return branchesContainingHead
    .map(branchLockfileName)
}

function branchLockfileName (branchName: string): string {
  return WANTED_LOCKFILE.replace('.yaml', `.${stringifyBranchName(branchName)}.yaml`)
}

/**
 * 1. Git branch name may contains slashes, which is not allowed in filenames
 * 2. Filesystem may be case-insensitive, so we need to convert branch name to lowercase
 */
function stringifyBranchName (branchName: string = ''): string {
  return branchName.replace(/[^\w.-]/g, '!').toLowerCase()
}
