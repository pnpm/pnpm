import path from 'path'
import { PnpmError } from '@pnpm/error'
import { type PackageManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import normalizePackageData from 'normalize-package-data'

type MaybeLibc = PackageManifest['libc']

function normalizeLibc (libc: MaybeLibc): string[] | undefined {
  if (libc == null) return undefined
  const libcArray = Array.isArray(libc) ? libc : [libc]
  const normalizedLibc = libcArray.filter((value): value is string => typeof value === 'string' && value.length > 0)
  return normalizedLibc.length > 0 ? normalizedLibc : undefined
}

function preserveLibcField (manifest: PackageManifest): void {
  const originalLibc = normalizeLibc(manifest.libc)
  normalizePackageData(manifest)
  const normalizedLibc = normalizeLibc(manifest.libc) ?? originalLibc
  if (normalizedLibc) {
    manifest.libc = normalizedLibc
  } else {
    delete manifest.libc
  }
}

export function readPackageJsonSync (pkgPath: string): PackageManifest {
  try {
    const manifest = loadJsonFile.sync<PackageManifest>(pkgPath)
    preserveLibcField(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export async function readPackageJson (pkgPath: string): Promise<PackageManifest> {
  try {
    const manifest = await loadJsonFile<PackageManifest>(pkgPath)
    preserveLibcField(manifest)
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
