import {PackageJson} from '@pnpm/types'
import path = require('path')
import readPackageJsonCB = require('read-package-json')
import promisify = require('util.promisify')

const readPackageJson = promisify(readPackageJsonCB)

export default function readPkg (pkgPath: string): Promise<PackageJson> {
  return readPackageJson(pkgPath)
}

export function fromDir (pkgPath: string): Promise<PackageJson> {
  return readPkg(path.join(pkgPath, 'package.json'))
}
