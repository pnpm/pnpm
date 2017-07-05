import path = require('path')
import {Package} from '../types'
import readPkg from './readPkg'

export default async function safeReadPkg (pkgPath: string): Promise<Package | null> {
  try {
    return await readPkg(pkgPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return null
  }
}

export function fromDir (pkgPath: string): Promise<Package | null> {
  return safeReadPkg(path.join(pkgPath, 'package.json'))
}
