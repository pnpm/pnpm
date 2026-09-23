import util from 'node:util'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getBranchCandidatesFromGit, getBranchFromCiEnv, getCurrentBranch } from '@pnpm/network.git-utils'

import { getGitBranchLockfileNames } from './gitBranchLockfile.js'

export interface GetWantedLockfileNameOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  cwd?: string
  lockfileDir?: string
}

export async function getWantedLockfileName (opts: GetWantedLockfileNameOptions = {}): Promise<string> {
  if (opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles) {
    let currentBranchName = await getCurrentBranch({ cwd: opts.cwd })
    if (!currentBranchName) {
      currentBranchName = getBranchFromCiEnv(opts.cwd)
    }
    if (!currentBranchName) {
      currentBranchName = await matchGitBranchLockfile({ cwd: opts.cwd, lockfileDir: opts.lockfileDir })
    }
    if (currentBranchName) {
      return WANTED_LOCKFILE.replace('.yaml', `.${stringifyBranchName(currentBranchName)}.yaml`)
    }
  }
  return WANTED_LOCKFILE
}

interface MatchGitBranchLockfileOptions {
  cwd?: string
  lockfileDir?: string
}

async function matchGitBranchLockfile (opts: MatchGitBranchLockfileOptions = {}): Promise<string | null> {
  const targetDir = opts.lockfileDir ?? opts.cwd ?? process.cwd()
  let existingLockfiles: string[]
  try {
    existingLockfiles = await getGitBranchLockfileNames(targetDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  if (existingLockfiles.length === 0) {
    return null
  }
  const existingSet = new Set(existingLockfiles)
  const candidates = await getBranchCandidatesFromGit({ cwd: opts.cwd })
  for (const candidate of candidates) {
    const filename = `pnpm-lock.${stringifyBranchName(candidate)}.yaml`
    if (existingSet.has(filename)) {
      return candidate
    }
  }
  return null
}

/**
 * 1. Git branch name may contains slashes, which is not allowed in filenames
 * 2. Filesystem may be case-insensitive, so we need to convert branch name to lowercase
 */
function stringifyBranchName (branchName: string = ''): string {
  return branchName.replace(/[^\w.-]/g, '!').toLowerCase()
}
