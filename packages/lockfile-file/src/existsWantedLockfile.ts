import fs from 'fs'
import path from 'path'
import { getWantedLockfileName } from './lockfileName'

interface ExistsWantedLockfileOptions {
  useGitBranchLockfile?: boolean
}

export default async (pkgPath: string, opts: ExistsWantedLockfileOptions = {
  useGitBranchLockfile: false,
}) => new Promise((resolve, reject) => {
  const wantedLockfile: string = getWantedLockfileName(opts)
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
