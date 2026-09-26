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

export interface WantedLockfileSelection {
  /** The lockfile of the checked-out branch, or `pnpm-lock.yaml`. */
  fileName: string
  /**
   * On a detached HEAD, the lockfiles of the branches containing the
   * checked-out commit, which the read tries before `fileName`. Empty when
   * HEAD is attached to a branch.
   */
  detachedHeadCandidates: string[]
}

/**
 * The files the wanted-lockfile read tries.
 *
 * A detached HEAD names no branch, but the checked-out commit still belongs
 * to the branches whose history includes it, so their lockfiles can satisfy
 * the checkout. `mergeGitBranchLockfiles` reads the shared lockfile and folds
 * every branch lockfile in, so it has no candidates.
 */
export async function selectWantedLockfile (opts: GetWantedLockfileNameOptions = {}): Promise<WantedLockfileSelection> {
  if (!opts.useGitBranchLockfile || opts.mergeGitBranchLockfiles) {
    return { fileName: WANTED_LOCKFILE, detachedHeadCandidates: [] }
  }
  const currentBranchName = await getCurrentBranch({ cwd: opts.cwd })
  if (currentBranchName) {
    return { fileName: branchLockfileName(currentBranchName), detachedHeadCandidates: [] }
  }
  const branchesContainingHead = await getBranchesContainingHead({ cwd: opts.cwd })
  return {
    fileName: WANTED_LOCKFILE,
    // A name longer than a filesystem allows cannot be on disk, and probing
    // it fails with ENAMETOOLONG instead of reporting it absent.
    detachedHeadCandidates: branchesContainingHead
      .map(branchLockfileName)
      .filter((fileName) => fileName.length <= MAX_FILE_NAME_LENGTH),
  }
}

const MAX_FILE_NAME_LENGTH = 255

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
