import path from 'node:path'

import { isError } from '@pnpm/error'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { PackageManifest } from '@pnpm/types'
import semver from 'semver'

import type { HoistableRootDep } from './hoistPeers.js'
import type { ImporterToResolve, PkgAddressOrLink } from './resolutionTypes.js'
import { unwrapPackageName } from './unwrapPackageName.js'

/**
 * Lists the workspace-root dependencies that `hoistPeers` may satisfy a
 * missing peer with. A root dependency reused from the lockfile skips full
 * resolution, so it has no address (or an address without a
 * normalizedBareSpecifier); its wanted specifier is used instead, so that
 * re-resolving with a lockfile hoists the same version as a fresh install of
 * the same manifest.
 */
export async function getHoistableRootDeps (
  rootImporter: ImporterToResolve | undefined,
  rootPkgAddresses: PkgAddressOrLink[]
): Promise<HoistableRootDep[]> {
  const wantedSpecifierByAlias = new Map<string, string>()
  for (const wantedDep of rootImporter?.wantedDependencies ?? []) {
    if (wantedDep.alias && wantedDep.bareSpecifier) {
      wantedSpecifierByAlias.set(wantedDep.alias, wantedDep.bareSpecifier)
    }
  }
  const rootDir = rootImporter?.options.prefix
  const rootDeps: HoistableRootDep[] = rootPkgAddresses.map((pkgAddress) => ({
    alias: pkgAddress.alias,
    pkgName: pkgAddress.pkg.name,
    normalizedBareSpecifier: pkgAddress.normalizedBareSpecifier ?? wantedSpecifierByAlias.get(pkgAddress.alias),
  }))
  const coveredAliases = new Set(rootDeps.map(({ alias }) => alias))
  for (const [alias, bareSpecifier] of wantedSpecifierByAlias) {
    if (coveredAliases.has(alias)) continue
    rootDeps.push({
      alias,
      pkgName: unwrapPackageName(alias, bareSpecifier).pkgName,
      normalizedBareSpecifier: bareSpecifier,
    })
  }
  return Promise.all(rootDeps.map(async (rootDep) => {
    if (rootDep.normalizedBareSpecifier == null || !isProjectRelativeSpecifier(rootDep.normalizedBareSpecifier)) {
      return rootDep
    }
    return pinProjectRelativeDepToItsVersion(rootDep, rootDir)
  }))
}

/**
 * `link:`, `file:`, and the path form of `workspace:` name a directory relative
 * to the project that declares them, so the root's specifier cannot be hoisted
 * verbatim — it would reach a different path from the importer the peer is
 * hoisted into, or nothing. A `workspace:` range is not path-relative: it
 * selects the same workspace package from every importer, so it needs none of
 * this.
 */
function isProjectRelativeSpecifier (bareSpecifier: string): boolean {
  return bareSpecifier.startsWith('link:') || bareSpecifier.startsWith('file:') || bareSpecifier.startsWith('workspace:.')
}

/**
 * Substitutes the linked package's own version for its path, so the root keeps
 * the authority over the peer that a registry dependency has, and the peer
 * resolves to the same package from every importer. The manifest is read from
 * disk rather than taken from `pkgAddress.pkg`, which a linked dependency
 * reused from the lockfile does not have — reading it makes a repeat install
 * hoist what a fresh install of the same manifest hoists. A target with no
 * manifest to read (a `file:` tarball, a path that does not exist) or no
 * version in it is not a candidate.
 */
async function pinProjectRelativeDepToItsVersion (
  rootDep: HoistableRootDep,
  rootDir: string | undefined
): Promise<HoistableRootDep> {
  const pathWithoutProtocol = rootDep.normalizedBareSpecifier!.slice(rootDep.normalizedBareSpecifier!.indexOf(':') + 1)
  const manifest = rootDir == null
    ? null
    : await readManifestOfLocalTarget(path.resolve(rootDir, pathWithoutProtocol))
  if (manifest?.version == null || semver.valid(manifest.version) == null) {
    return { ...rootDep, normalizedBareSpecifier: undefined }
  }
  return {
    alias: rootDep.alias,
    pkgName: manifest.name ?? rootDep.pkgName,
    normalizedBareSpecifier: manifest.version,
  }
}

async function readManifestOfLocalTarget (dir: string): Promise<PackageManifest | null> {
  try {
    return await safeReadPackageJsonFromDir(dir)
  } catch (err: unknown) {
    // A `file:` target is a tarball as often as a directory, and a path
    // component of a tarball is not a directory to read a manifest from.
    if (isError(err) && 'code' in err && err.code === 'ENOTDIR') return null
    throw err
  }
}
