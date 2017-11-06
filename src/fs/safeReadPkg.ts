import path = require('path')
import {PackageJson} from '@pnpm/types'
import readPkg from './readPkg'

export default async function safeReadPkg (pkgPath: string): Promise<PackageJson | null> {
  try {
    return await readPkg(pkgPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return null
  }
}

export function fromDir (pkgPath: string): Promise<PackageJson | null> {
  return safeReadPkg(path.join(pkgPath, 'package.json'))
}
