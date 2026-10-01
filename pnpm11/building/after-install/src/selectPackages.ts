import { WANTED_LOCKFILE } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import { nameVerFromPkgSnapshot, type PackageSnapshots } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import npa from '@pnpm/npm-package-arg'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'
import semver from 'semver'

export type PackageSelector = string | {
  name: string
  range: string
} | {
  /** A user-written depPath spec, normalized with the peer suffix stripped. */
  pkgIdWithPatchHash: string
}

export function parsePackageSelectors (packages: PackageSnapshots, pkgSpecs: string[]): PackageSelector[] {
  return pkgSpecs.map((arg) => {
    if (matchesDepPath(packages, arg)) {
      return { pkgIdWithPatchHash: dp.removePeersSuffix(arg) }
    }
    const { fetchSpec, name, raw, type } = npa(arg)
    if (raw === name) {
      return name
    }
    if (type !== 'version' && type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. Rebuild can only select by version or range`)
    }
    return {
      name,
      range: fetchSpec,
    }
  })
}

function matchesDepPath (packages: PackageSnapshots, pkgSpec: string): boolean {
  const normalizedPkgSpec = dp.removePeersSuffix(pkgSpec)
  return Object.keys(packages).some((depPath) => dp.removePeersSuffix(depPath) === normalizedPkgSpec)
}

export function findPackages (
  packages: PackageSnapshots,
  searched: PackageSelector[],
  opts: {
    prefix: string
  }
): DepPath[] {
  return (Object.keys(packages) as DepPath[])
    .filter((relativeDepPath) => {
      const pkgLockfile = packages[relativeDepPath]
      const pkgInfo = nameVerFromPkgSnapshot(relativeDepPath, pkgLockfile)
      if (!pkgInfo.name) {
        logger.warn({
          message: `Skipping ${relativeDepPath} because cannot get the package name from ${WANTED_LOCKFILE}.
            Try to run \`pnpm update --depth 100\` to create a new ${WANTED_LOCKFILE} with all the necessary info.`,
          prefix: opts.prefix,
        })
        return false
      }
      return matches(searched, pkgInfo, dp.getPkgIdWithPatchHash(relativeDepPath))
    })
}

// TODO: move this logic to separate package as this is also used in tree-builder
function matches (
  searched: PackageSelector[],
  manifest: { name: string, version?: string },
  pkgIdWithPatchHash: PkgIdWithPatchHash
): boolean {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return manifest.name === searchedPkg
    }
    if ('pkgIdWithPatchHash' in searchedPkg) {
      return searchedPkg.pkgIdWithPatchHash === pkgIdWithPatchHash
    }
    return searchedPkg.name === manifest.name && !!manifest.version &&
      semver.satisfies(manifest.version, searchedPkg.range)
  })
}
