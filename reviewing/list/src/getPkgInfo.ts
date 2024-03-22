import path from 'node:path'

import type { PackageManifest, PkgData, PkgInfo } from '@pnpm/types'

import { readPkg } from './readPkg.js'

export async function getPkgInfo(pkg: PkgData): Promise<PkgInfo> {
  let manifest: PackageManifest

  try {
    manifest = await readPkg(path.join(pkg.path, 'package.json'))
  } catch (err: unknown) {
    // This will probably never happen
    // @ts-ignore
    manifest = {
      description: '[Could not find additional info about this dependency]',
    }
  }

  return {
    alias: pkg.alias,
    from: pkg.name,

    version: pkg.version,

    resolved: pkg.resolved,

    description: manifest.description,
    license: manifest.license,
    author: manifest.author,

    homepage: manifest.homepage,
    repository:
      (manifest.repository &&
        (typeof manifest.repository === 'string'
          ? manifest.repository
          : manifest.repository.url)) ??
      undefined,
    path: pkg.path,
  }
}
