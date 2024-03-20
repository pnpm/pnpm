import '@total-typescript/ts-reset'

import path from 'node:path'

import loadJsonFile from 'load-json-file'
import normalizePackageData from 'normalize-package-data'

import { PnpmError } from '@pnpm/error'
import type { PackageManifest } from '@pnpm/types'

export async function readPackageJson(
  pkgPath: string
): Promise<PackageManifest> {
  try {
    const manifest = await loadJsonFile<PackageManifest>(pkgPath)

    normalizePackageData(manifest)

    return manifest
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code) throw err
    throw new PnpmError(
      'BAD_PACKAGE_JSON',
      // @ts-ignore
      `${pkgPath}: ${err.message}`
    )
  }
}

export async function readPackageJsonFromDir(
  pkgPath: string
): Promise<PackageManifest> {
  return readPackageJson(path.join(pkgPath, 'package.json'))
}

export async function safeReadPackageJson(
  pkgPath: string
): Promise<PackageManifest | null> {
  try {
    return await readPackageJson(pkgPath)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }

    return null
  }
}

export async function safeReadPackageJsonFromDir(
  pkgPath: string
): Promise<PackageManifest | null> {
  return safeReadPackageJson(path.join(pkgPath, 'package.json'))
}
