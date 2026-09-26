import fs from 'node:fs'
import path from 'node:path'

import { selectWantedLockfile } from './lockfileName.js'

interface ExistsNonEmptyWantedLockfileOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export async function existsNonEmptyWantedLockfile (pkgPath: string, opts: ExistsNonEmptyWantedLockfileOptions = {
  useGitBranchLockfile: false,
  mergeGitBranchLockfiles: false,
}): Promise<boolean> {
  const { fileName, detachedHeadCandidates } = await selectWantedLockfile(opts)
  if (detachedHeadCandidates.length === 0) {
    return fileExists(path.join(pkgPath, fileName))
  }
  /* eslint-disable no-await-in-loop */
  for (const candidate of [...detachedHeadCandidates, fileName]) {
    if (await fileExists(path.join(pkgPath, candidate))) return true
  }
  /* eslint-enable no-await-in-loop */
  return false
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
