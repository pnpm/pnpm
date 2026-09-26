import fs from 'node:fs'
import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'

import { getWantedLockfileNames } from './lockfileName.js'

interface ExistsNonEmptyWantedLockfileOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export async function existsNonEmptyWantedLockfile (pkgPath: string, opts: ExistsNonEmptyWantedLockfileOptions = {
  useGitBranchLockfile: false,
  mergeGitBranchLockfiles: false,
}): Promise<boolean> {
  const wantedLockfileNames: string[] = await getWantedLockfileNames(opts)
  // On a detached HEAD the read falls back to the lockfiles of the branches
  // containing the commit; any of them counts, as does the shared lockfile
  // they are tried before. Everywhere else the branch lockfile, when one is
  // named, is the only file that counts.
  const names = wantedLockfileNames.length > 0
    ? [...wantedLockfileNames, WANTED_LOCKFILE]
    : [WANTED_LOCKFILE]
  const existence = await Promise.all(names.map((name) => fileExists(path.join(pkgPath, name))))
  return existence.includes(true)
}

function fileExists (filePath: string): Promise<boolean> {
  return new Promise((resolve, reject) => {
    fs.access(filePath, (err) => {
      if (err == null) {
        resolve(true)
        return
      }
      if (err.code === 'ENOENT') {
        resolve(false)
        return
      }
      reject(err)
    })
  })
}
