import fs from 'fs'
import path from 'path'
import { getWantedLockfileName } from './lockfileName'

interface ExistsWantedLockfileOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export default async (pkgPath: string, opts: ExistsWantedLockfileOptions = {
  useGitBranchLockfile: false,
  mergeGitBranchLockfiles: false,
}) => {
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
