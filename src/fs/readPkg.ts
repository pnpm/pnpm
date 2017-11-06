import path = require('path')
import {PackageJson} from '@pnpm/types'
import readPackageJsonCB = require('read-package-json')
import thenify = require('thenify')

const readPackageJson = thenify(readPackageJsonCB)

export default function readPkg (pkgPath: string): Promise<PackageJson> {
  return readPackageJson(pkgPath)
}

export function fromDir (pkgPath: string): Promise<PackageJson> {
  return readPkg(path.join(pkgPath, 'package.json'))
}
