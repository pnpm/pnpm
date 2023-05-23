import fs from 'fs'
import path from 'path'
import { getWantedLockfileName } from './lockfileName'

interface existsNonEmptyWantedLockfileOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export async function existsNonEmptyWantedLockfile (pkgPath: string, opts: existsNonEmptyWantedLockfileOptions = {
  useGitBranchLockfile: false,
  mergeGitBranchLockfiles: false,
}) {
  const wantedLockfile: string = await getWantedLockfileName(opts)
  return new Promise((resolve, reject) => {
    fs.access(path.join(pkgPath, wantedLockfile), (err) => {
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
