import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { PackageResponse, StoreController } from '@pnpm/store.controller-types'
import type { PackageManifest, PkgResolutionId, ProjectId, ProjectRootDir } from '@pnpm/types'

import { type ImporterToResolveGeneric, type ResolveDependenciesOptions, resolveDependencyTree } from '../lib/resolveDependencyTree.js'

test('shared package children are resolved from the deterministic shallowest context', async () => {
  const requestLog: Array<{
    alias: string
    bareSpecifier: string
    resolvedId: string
  }> = []
  const storeController = createStoreController(async (wantedDependency, options) => {
    if (wantedDependency.alias === 'shared' && !hasPreferredVersion(options.preferredVersions, 'provider', '1.0.0')) {
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    const alias = wantedDependency.alias!
    const bareSpecifier = wantedDependency.bareSpecifier!
    const version = pickVersion(alias, bareSpecifier, options.preferredVersions)
    const resolvedId = `${alias}@${version}`
    requestLog.push({
      alias,
      bareSpecifier,
      resolvedId,
    })
    return createPackageResponse(resolvedId)
  })
  const lockfile = createLockfile()

  const result = await resolveDependencyTree([
    {
      id: '.' as ProjectId,
      manifest: {
        name: 'root',
        version: '0.0.0',
        dependencies: {
          a: '1.0.0',
          c: '1.0.0',
        },
      },
      modulesDir: '/project/node_modules',
      rootDir: '/project' as ProjectRootDir,
      updatePackageManifest: false,
      wantedDependencies: [
        {
          alias: 'a',
          bareSpecifier: '1.0.0',
          dev: false,
          optional: false,
          updateDepth: 0,
        },
        {
          alias: 'c',
          bareSpecifier: '1.0.0',
          dev: false,
          optional: false,
          updateDepth: 0,
        },
      ],
    } satisfies ImporterToResolveGeneric<object>,
  ], {
    allowedDeprecatedVersions: {},
    allowUnusedPatches: false,
    currentLockfile: lockfile,
    dryRun: false,
    engineStrict: false,
    force: false,
    forceFullResolution: false,
    hooks: {},
    lockfileDir: '/project',
    pnpmVersion: '0.0.0',
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    storeController,
    tag: 'latest',
    virtualStoreDir: '/project/node_modules/.pnpm',
    globalVirtualStoreDir: '/project/node_modules/.pnpm/global',
    virtualStoreDirMaxLength: 120,
    wantedLockfile: lockfile,
    workspacePackages: new Map(),
    peersSuffixMaxLength: 1000,
    dedupePeerDependents: true,
  } satisfies ResolveDependenciesOptions)

  expect(requestLog
    .filter(({ alias, bareSpecifier }) => alias === 'provider' && bareSpecifier === '*')
    .map(({ resolvedId }) => resolvedId)
  ).toStrictEqual(['provider@2.0.0'])

  const sharedChildren = Array.from(result.dependenciesTree.values())
    .filter(({ resolvedPackage }) => resolvedPackage.name === 'shared')
    .map((node) => typeof node.children === 'function' ? node.children() : node.children)

  expect(sharedChildren).toHaveLength(2)
  expect(sharedChildren.map(({ provider }) => provider)).toStrictEqual([
    'provider@2.0.0',
    'provider@2.0.0',
  ])
})

function hasPreferredVersion (preferredVersions: PreferredVersions, alias: string, version: string): boolean {
  return Boolean(preferredVersions[alias]?.[version])
}

function pickVersion (alias: string, bareSpecifier: string, preferredVersions: PreferredVersions): string {
  if (alias === 'provider' && bareSpecifier === '*') {
    return hasPreferredVersion(preferredVersions, alias, '1.0.0') ? '1.0.0' : '2.0.0'
  }
  return bareSpecifier
}

function createLockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
      },
    },
    packages: {},
  }
}

function createPackageResponse (pkgId: string): PackageResponse {
  const manifest = manifests[pkgId]
  return {
    body: {
      id: pkgId as PkgResolutionId,
      isLocal: false,
      manifest,
      resolution: {
        tarball: `https://registry.npmjs.org/${manifest.name}/-/${manifest.name}-${manifest.version}.tgz`,
      },
      updated: false,
    },
    fetching: async () => ({
      files: {
        filesMap: new Map(),
        requiresBuild: false,
        resolvedFrom: 'remote',
      },
    }),
    filesIndexFile: `${pkgId}-index.json`,
  }
}

function createStoreController (
  requestPackage: StoreController['requestPackage']
): StoreController {
  return {
    requestPackage,
    fetchPackage: () => {
      throw new Error('fetchPackage should not be called')
    },
    getFilesIndexFilePath: () => ({
      filesIndexFile: '',
      target: '',
    }),
    importPackage: async () => ({ isBuilt: false, importMethod: undefined }),
    close: async () => undefined,
    prune: async () => undefined,
    upload: async () => undefined,
    clearResolutionCache: () => undefined,
  }
}

const manifests: Record<string, PackageManifest> = {
  'a@1.0.0': {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      provider: '1.0.0',
      b: '1.0.0',
    },
  },
  'b@1.0.0': {
    name: 'b',
    version: '1.0.0',
    dependencies: {
      shared: '1.0.0',
    },
  },
  'c@1.0.0': {
    name: 'c',
    version: '1.0.0',
    dependencies: {
      shared: '1.0.0',
    },
  },
  'provider@1.0.0': {
    name: 'provider',
    version: '1.0.0',
  },
  'provider@2.0.0': {
    name: 'provider',
    version: '2.0.0',
  },
  'shared@1.0.0': {
    name: 'shared',
    version: '1.0.0',
    dependencies: {
      provider: '*',
    },
  },
}
