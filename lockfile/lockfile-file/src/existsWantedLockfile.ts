import fs from 'node:fs'
import path from 'node:path'

import { getWantedLockfileName } from './lockfileName.js'

type existsNonEmptyWantedLockfileOptions = {
  useGitBranchLockfile?: boolean | undefined
  mergeGitBranchLockfiles?: boolean | undefined
}

export async function existsNonEmptyWantedLockfile(
  pkgPath: string,
  opts: existsNonEmptyWantedLockfileOptions | undefined = {
    useGitBranchLockfile: false,
    mergeGitBranchLockfiles: false,
  }
): Promise<boolean> {
  const wantedLockfile: string = await getWantedLockfileName(opts)

  return new Promise((resolve, reject): void => {
    fs.access(path.join(pkgPath, wantedLockfile), (err: NodeJS.ErrnoException | null): void => {
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
