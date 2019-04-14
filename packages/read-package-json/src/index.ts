import { PackageJson } from '@pnpm/types'
import path = require('path')
import readPackageJsonCB = require('read-package-json')
import { promisify } from 'util'

const readPackageJson = promisify(readPackageJsonCB)

export default async function readPkg (pkgPath: string): Promise<PackageJson> {
  try {
    return await readPackageJson(pkgPath)
  } catch (err) {
    if (err['code']) throw err // tslint:disable-line
    const pnpmError = new Error(`${pkgPath}: ${err.message}`)
    pnpmError['code'] = 'ERR_PNPM_BAD_PACKAGE_JSON' // tslint:disable-line
    throw pnpmError
  }
}

export function fromDir (pkgPath: string): Promise<PackageJson> {
  return readPkg(path.join(pkgPath, 'package.json'))
}
