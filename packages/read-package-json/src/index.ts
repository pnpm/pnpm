import { PackageManifest } from '@pnpm/types'
import path = require('path')
import readPackageManifestCB = require('read-package-json')
import { promisify } from 'util'

const readPackageManifest = promisify(readPackageManifestCB)

export default async function readPkg (pkgPath: string): Promise<PackageManifest> {
  try {
    return await readPackageManifest(pkgPath)
  } catch (err) {
    if (err['code']) throw err // tslint:disable-line
    const pnpmError = new Error(`${pkgPath}: ${err.message}`)
    pnpmError['code'] = 'ERR_PNPM_BAD_PACKAGE_JSON' // tslint:disable-line
    throw pnpmError
  }
}

export function fromDir (pkgPath: string): Promise<PackageManifest> {
  return readPkg(path.join(pkgPath, 'package.json'))
}
