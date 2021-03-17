import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'

export default async (pkgPath: string) => new Promise((resolve, reject) => {
  fs.access(path.join(pkgPath, WANTED_LOCKFILE), (err) => {
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
