import path from 'path'
import PnpmError from '@pnpm/error'
import { PackageManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import normalizePackageData from 'normalize-package-data'

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

export async function fromDir (pkgPath: string): Promise<PackageManifest> {
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

export async function safeReadPackageFromDir (pkgPath: string): Promise<PackageManifest | null> {
  return safeReadPackage(path.join(pkgPath, 'package.json'))
}
