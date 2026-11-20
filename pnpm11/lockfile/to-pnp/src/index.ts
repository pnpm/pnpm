import { promises as fs } from 'node:fs'
import path from 'node:path'

import { depPathToFilename, packageRootLinkTarget, refToRelative } from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import type { RegistriesByScope } from '@pnpm/types'
import { generateInlinedScript, type PackageRegistry } from '@yarnpkg/pnp'
import normalizePath from 'normalize-path'

export {
  type DependenciesGraphPackageMapOptions,
  dependenciesGraphToPackageMap,
  lockfileToPackageMap,
  PACKAGE_MAP_FILENAME,
  type PackageMap,
  type PackageMapGraphNode,
  type PackageMapOptions,
  type PackageMapPackage,
  type PackageMapType,
  removePackageMap,
  writePackageMap,
  writePackageMapFromDependenciesGraph,
} from './packageMap.js'

export async function writePnpFile (
  lockfile: LockfileObject,
  opts: {
    importerNames: Record<string, string>
    lockfileDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    registriesByScope: RegistriesByScope
  }
): Promise<void> {
  const packageRegistry = lockfileToPackageRegistry(lockfile, opts)

  const loaderFile = generateInlinedScript({
    dependencyTreeRoots: [],
    ignorePattern: undefined,
    packageRegistry,
    pnpZipBackend: 'libzip',
    shebang: undefined,
  })
  await fs.writeFile(path.join(opts.lockfileDir, '.pnp.cjs'), loaderFile, 'utf8')
}

interface PackageRegistryOptions {
  importerNames: { [importerId: string]: string }
  lockfileDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  registriesByScope: RegistriesByScope
}

type PackageDependencyReference = string | [string, string]

interface PackageStoreEntry {
  packageDependencies: Map<string, PackageDependencyReference>
  packageLocation: string
}

interface PackageRegistryContext {
  lockfile: LockfileObject
  opts: PackageRegistryOptions
  packageRegistry: Map<string, Map<string, PackageStoreEntry>>
}

export function lockfileToPackageRegistry (
  lockfile: LockfileObject,
  opts: PackageRegistryOptions
): PackageRegistry {
  const packageRegistry = new Map()
  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    if (importerId === '.') {
      packageRegistry.set(null, createRootImporterPackageStore(lockfile, importer))
    } else {
      const name = opts.importerNames[importerId]
      packageRegistry.set(name, createImporterPackageStore(lockfile, importer, { id: importerId, name }))
    }
  }
  const ctx: PackageRegistryContext = { lockfile, opts, packageRegistry }
  for (const [relDepPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {})) {
    registerLockfilePackage(ctx, relDepPath, pkgSnapshot)
  }

  return packageRegistry
}

function createRootImporterPackageStore (
  lockfile: LockfileObject,
  importer: ProjectSnapshot
): Map<null, PackageStoreEntry> {
  return new Map([
    [
      null,
      {
        packageDependencies: new Map(importerDependencies(lockfile, importer)),
        packageLocation: './',
      },
    ],
  ])
}

function createImporterPackageStore (
  lockfile: LockfileObject,
  importer: ProjectSnapshot,
  project: { id: string, name: string }
): Map<string, PackageStoreEntry> {
  return new Map([
    [
      project.id,
      {
        packageDependencies: new Map<string, PackageDependencyReference>([
          [project.name, project.id],
          ...importerDependencies(lockfile, importer, project.id),
        ]),
        packageLocation: `./${project.id}/`,
      },
    ],
  ])
}

function importerDependencies (
  lockfile: LockfileObject,
  importer: ProjectSnapshot,
  importerId?: string
): Array<[string, PackageDependencyReference]> {
  return [
    ...((importer.dependencies != null) ? toPackageDependenciesMap(lockfile, importer.dependencies, importerId) : []),
    ...((importer.optionalDependencies != null) ? toPackageDependenciesMap(lockfile, importer.optionalDependencies, importerId) : []),
    ...((importer.devDependencies != null) ? toPackageDependenciesMap(lockfile, importer.devDependencies, importerId) : []),
  ]
}

function registerLockfilePackage (
  ctx: PackageRegistryContext,
  relDepPath: string,
  pkgSnapshot: PackageSnapshot
): void {
  const { lockfile, packageRegistry } = ctx
  const { name, version, peerDepGraphHash } = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
  const pnpVersion = toPnPVersion(version, peerDepGraphHash)
  const packageStore = getOrCreatePackageStore(packageRegistry, name)
  const packageLocation = toPackageLocation(ctx.opts, relDepPath, name)
  const packageRootLinks = registerPackageRootLinks(packageRegistry, packageLocation, {
    ...pkgSnapshot.dependencies,
    ...pkgSnapshot.optionalDependencies,
  })
  packageStore.set(pnpVersion, {
    packageDependencies: new Map([
      [name, pnpVersion],
      ...((pkgSnapshot.dependencies != null) ? toPackageDependenciesMap(lockfile, pkgSnapshot.dependencies) : []),
      ...((pkgSnapshot.optionalDependencies != null) ? toPackageDependenciesMap(lockfile, pkgSnapshot.optionalDependencies) : []),
      ...packageRootLinks,
    ]),
    packageLocation,
  })
}

function toPackageLocation (opts: PackageRegistryOptions, relDepPath: string, name: string): string {
  // Seems like this field should always contain a relative path.
  // `name` is reconstructed from the (attacker-controllable) lockfile depPath
  // key via `dp.parse`, which does no validation, so contain it here to keep
  // the PnP resolver map from pointing outside the virtual store.
  const pkgModulesDir = path.join(
    opts.virtualStoreDir,
    depPathToFilename(relDepPath, opts.virtualStoreDirMaxLength),
    'node_modules'
  )
  let packageLocation = normalizePath(path.relative(opts.lockfileDir, safeJoinModulesDir(pkgModulesDir, name)))
  if (!packageLocation.startsWith('../')) {
    packageLocation = `./${packageLocation}`
  }
  if (!packageLocation.endsWith('/')) {
    packageLocation += '/'
  }
  return packageLocation
}

function getOrCreatePackageStore (
  packageRegistry: Map<string, Map<string, PackageStoreEntry>>,
  name: string
): Map<string, PackageStoreEntry> {
  let packageStore = packageRegistry.get(name)
  if (!packageStore) {
    packageStore = new Map()
    packageRegistry.set(name, packageStore)
  }
  return packageStore
}

/**
 * Registers each `link:<root>/...` dependency as a package located inside the
 * package at `packageLocation`, and returns the references to depend on it.
 */
function registerPackageRootLinks (
  packageRegistry: Map<string, Map<string, PackageStoreEntry>>,
  packageLocation: string,
  deps: Record<string, string>
): Array<[string, string]> {
  const links: Array<[string, string]> = []
  for (const [alias, ref] of Object.entries(deps)) {
    const target = packageRootLinkTarget(ref)
    if (target == null) continue
    const linkLocation = `${packageLocation}${target}/`
    getOrCreatePackageStore(packageRegistry, alias).set(linkLocation, {
      packageDependencies: new Map([[alias, linkLocation]]),
      packageLocation: linkLocation,
    })
    links.push([alias, linkLocation])
  }
  return links
}

function toPackageDependenciesMap (
  lockfile: LockfileObject,
  deps: {
    [depAlias: string]: string
  },
  importerId?: string
): Array<[string, PackageDependencyReference]> {
  return Object.entries(deps).map(([depAlias, ref]) => {
    if (importerId && ref.startsWith('link:')) {
      return [depAlias, normalizePath(path.join(importerId, ref.slice(5)))]
    }
    const relDepPath = refToRelative(ref, depAlias)
    if (!relDepPath) return [depAlias, ref]
    const { name, version, peerDepGraphHash } = nameVerFromPkgSnapshot(relDepPath, lockfile.packages![relDepPath])
    const pnpVersion = toPnPVersion(version, peerDepGraphHash)
    if (depAlias === name) {
      return [depAlias, pnpVersion]
    }
    return [depAlias, [name, pnpVersion]]
  })
}

function toPnPVersion (version: string, peerDepGraphHash: string | undefined) {
  return peerDepGraphHash
    ? `virtual:${version}${peerDepGraphHash}#${version}`
    : version
}
