import PnpmError from '@pnpm/error'
import { PackageManifest } from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import normalizePackageData = require('normalize-package-data')
import path = require('path')

export default async function readPkg (pkgPath: string): Promise<PackageManifest> {
  try {
    const manifest = await loadJsonFile<PackageManifest>(pkgPath)
    normalizePackageData(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export function fromDir (pkgPath: string): Promise<PackageManifest> {
  return readPkg(path.join(pkgPath, 'package.json'))
}

export async function safeReadPackage (pkgPath: string): Promise<PackageManifest | null> {
  try {
    return await readPkg(pkgPath)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return null
  }
}

export function safeReadPackageFromDir (pkgPath: string): Promise<PackageManifest | null> {
  return safeReadPackage(path.join(pkgPath, 'package.json'))
}
