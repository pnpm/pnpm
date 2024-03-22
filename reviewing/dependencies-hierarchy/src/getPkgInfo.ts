import path from 'node:path'

import normalizePath from 'normalize-path'

import type {
  PackageInfo,
  PackageSnapshot,
  TarballResolution,
  GetPkgInfoOpts,
} from '@pnpm/types'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { depPathToFilename, refToRelative } from '@pnpm/dependency-path'

export function getPkgInfo(opts: GetPkgInfoOpts): PackageInfo {
  let name!: string

  let version: string

  let resolved: string | undefined

  let dev: boolean | undefined

  let optional: boolean | undefined

  let isSkipped: boolean = false

  let isMissing: boolean = false

  const depPath = refToRelative(opts.ref, opts.alias)

  if (depPath) {
    let pkgSnapshot!: PackageSnapshot | undefined

    if (opts.currentPackages[depPath]) {
      pkgSnapshot = opts.currentPackages[depPath]

      const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

      name = parsed.name

      version = parsed.version
    } else {
      pkgSnapshot = opts.wantedPackages[depPath]

      if (pkgSnapshot) {
        const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

        name = parsed.name

        version = parsed.version
      } else {
        name = opts.alias

        version = opts.ref
      }

      isMissing = true

      isSkipped = opts.skipped.has(depPath)
    }

    resolved = (
      pkgSnapshotToResolution(
        depPath,
        pkgSnapshot,
        opts.registries
      )
    ).tarball

    dev = pkgSnapshot?.dev

    optional = pkgSnapshot?.optional
  } else {
    name = opts.alias

    version = opts.ref
  }

  if (!version) {
    version = opts.ref
  }

  const fullPackagePath = depPath
    ? path.join(
      opts.virtualStoreDir ?? '.pnpm',
      depPathToFilename(depPath),
      'node_modules',
      name
    )
    : path.join(opts.linkedPathBaseDir, opts.ref.slice(5))

  if (version.startsWith('link:') && opts.rewriteLinkVersionDir) {
    version = `link:${normalizePath(path.relative(opts.rewriteLinkVersionDir, fullPackagePath))}`
  }

  const packageInfo: PackageInfo = {
    alias: opts.alias,
    isMissing,
    isPeer: Boolean(opts.peers?.has(opts.alias)),
    isSkipped,
    name,
    path: fullPackagePath,
    version,
  }

  if (resolved) {
    packageInfo.resolved = resolved
  }

  if (optional === true) {
    packageInfo.optional = true
  }

  if (typeof dev === 'boolean') {
    packageInfo.dev = dev
  }

  return packageInfo
}
