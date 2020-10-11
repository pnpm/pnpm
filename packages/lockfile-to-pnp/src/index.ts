import getConfigs from '@pnpm/config'
import { Lockfile, readWantedLockfile } from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import readImporterManifest from '@pnpm/read-project-manifest'
import { Registries } from '@pnpm/types'
import { refToRelative } from 'dependency-path'
import { generateInlinedScript, PackageRegistry } from '@yarnpkg/pnp'
import fs = require('mz/fs')
import normalizePath = require('normalize-path')
import path = require('path')
import R = require('ramda')

export async function lockfileToPnp (lockfileDir: string) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (!lockfile) throw new Error('Cannot generate a .pnp.js without a lockfile')
  const importerNames: { [importerId: string]: string } = {}
  await Promise.all(
    Object.keys(lockfile.importers)
      .map(async (importerId) => {
        const importerDirectory = path.join(lockfileDir, importerId)
        const { manifest } = await readImporterManifest(importerDirectory)
        importerNames[importerId] = manifest.name as string
      })
  )
  const { config: { registries, virtualStoreDir } } = await getConfigs({
    cliOptions: {},
    packageManager: { name: 'pnpm', version: '*' },
  })
  await writePnpFile(lockfile, {
    importerNames,
    lockfileDir,
    registries,
    virtualStoreDir: virtualStoreDir ?? path.join(lockfileDir, 'node_modules/.pnpm'),
  })
}

export async function writePnpFile (
  lockfile: Lockfile,
  opts: {
    importerNames: Record<string, string>
    lockfileDir: string
    virtualStoreDir: string
    registries: Registries
  }
) {
  const packageRegistry = lockfileToPackageRegistry(lockfile, opts)

  const loaderFile = generateInlinedScript({
    blacklistedLocations: undefined,
    dependencyTreeRoots: [],
    ignorePattern: undefined,
    packageRegistry,
    shebang: undefined,
  })
  await fs.writeFile(path.join(opts.lockfileDir, '.pnp.js'), loaderFile, 'utf8')
}

export function lockfileToPackageRegistry (
  lockfile: Lockfile,
  opts: {
    importerNames: { [importerId: string]: string }
    lockfileDir: string
    virtualStoreDir: string
    registries: Registries
  }
): PackageRegistry {
  const packageRegistry = new Map()
  for (const importerId of Object.keys(lockfile.importers)) {
    const importer = lockfile.importers[importerId]
    if (importerId === '.') {
      const packageStore = new Map([
        [
          null,
          {
            packageDependencies: new Map([
              ...((importer.dependencies && toPackageDependenciesMap(lockfile, importer.dependencies)) ?? []),
              ...((importer.optionalDependencies && toPackageDependenciesMap(lockfile, importer.optionalDependencies)) ?? []),
              ...((importer.devDependencies && toPackageDependenciesMap(lockfile, importer.devDependencies)) ?? []),
            ]),
            packageLocation: './',
          },
        ],
      ])
      packageRegistry.set(null, packageStore)
    } else {
      const name = opts.importerNames[importerId]
      const packageStore = new Map([
        [
          importerId,
          {
            packageDependencies: new Map([
              [name, importerId],
              ...((importer.dependencies && toPackageDependenciesMap(lockfile, importer.dependencies, importerId)) ?? []),
              ...((importer.optionalDependencies && toPackageDependenciesMap(lockfile, importer.optionalDependencies, importerId)) ?? []),
              ...((importer.devDependencies && toPackageDependenciesMap(lockfile, importer.devDependencies, importerId)) ?? []),
            ]),
            packageLocation: `./${importerId}`,
          },
        ],
      ])
      packageRegistry.set(name, packageStore)
    }
  }
  for (const [relDepPath, pkgSnapshot] of R.toPairs(lockfile.packages ?? {})) {
    const { name, version, peersSuffix } = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
    const pnpVersion = toPnPVersion(version, peersSuffix)
    let packageStore = packageRegistry.get(name)
    if (!packageStore) {
      packageStore = new Map()
      packageRegistry.set(name, packageStore)
    }

    // Seems like this field should always contain a relative path
    let packageLocation = normalizePath(path.relative(opts.lockfileDir, path.join(
      opts.virtualStoreDir,
      pkgIdToFilename(relDepPath, opts.lockfileDir),
      'node_modules',
      name
    )))
    if (!packageLocation.startsWith('../')) {
      packageLocation = `./${packageLocation}`
    }
    packageStore.set(pnpVersion, {
      packageDependencies: new Map([
        [name, pnpVersion],
        ...((pkgSnapshot.dependencies && toPackageDependenciesMap(lockfile, pkgSnapshot.dependencies)) ?? []),
        ...((pkgSnapshot.optionalDependencies && toPackageDependenciesMap(lockfile, pkgSnapshot.optionalDependencies)) ?? []),
      ]),
      packageLocation,
    })
  }

  return packageRegistry
}

function toPackageDependenciesMap (
  lockfile: Lockfile,
  deps: {
    [depAlias: string]: string
  },
  importerId?: string
): Array<[string, string | [string, string]]> {
  return R.toPairs(deps).map(([depAlias, ref]) => {
    if (importerId && ref.startsWith('link:')) {
      return [depAlias, path.join(importerId, ref.substr(5))]
    }
    const relDepPath = refToRelative(ref, depAlias)
    if (!relDepPath) return [depAlias, ref]
    const { name, version, peersSuffix } = nameVerFromPkgSnapshot(relDepPath, lockfile.packages![relDepPath])
    const pnpVersion = toPnPVersion(version, peersSuffix)
    if (depAlias === name) {
      return [depAlias, pnpVersion]
    }
    return [depAlias, [name, pnpVersion]]
  })
}

function toPnPVersion (version: string, peersSuffix: string | undefined) {
  return peersSuffix
    ? `virtual:${version}_${peersSuffix}#${version}`
    : version
}
