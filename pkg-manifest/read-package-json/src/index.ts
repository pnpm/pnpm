import path from 'path'
import util from 'util'
import { PnpmError } from '@pnpm/error'
import { type PackageManifest } from '@pnpm/types'
import { loadJsonFile, loadJsonFileSync } from 'load-json-file'
import normalizePackageData from 'normalize-package-data'

export function readPackageJsonSync (pkgPath: string): PackageManifest {
  try {
    const manifest = loadJsonFileSync<PackageManifest>(pkgPath)
    normalizePackageData(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export async function readPackageJson (pkgPath: string): Promise<PackageManifest> {
  try {
    const manifest = await loadJsonFile<PackageManifest>(pkgPath)
    normalizePackageData(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export function readPackageJsonFromDirSync (pkgPath: string): PackageManifest {
  return readPackageJsonSync(path.join(pkgPath, 'package.json'))
}

export async function readPackageJsonFromDir (pkgPath: string): Promise<PackageManifest> {
  return readPackageJson(path.join(pkgPath, 'package.json'))
}

export async function safeReadPackageJson (pkgPath: string): Promise<PackageManifest | null> {
  try {
    return await readPackageJson(pkgPath)
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return null
  }
}

export async function safeReadPackageJsonFromDir (pkgPath: string): Promise<PackageManifest | null> {
  return safeReadPackageJson(path.join(pkgPath, 'package.json'))
}

export function readPackageJsonFromDirRawSync (pkgPath: string): PackageManifest {
  try {
    return loadJsonFileSync<PackageManifest>(path.join(pkgPath, 'package.json'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err instanceof Error ? err.message : String(err)}`)
  }
}
