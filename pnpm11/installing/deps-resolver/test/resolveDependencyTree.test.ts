import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { PackageResponse, StoreController } from '@pnpm/store.controller-types'
import type { DepPath, PackageManifest, PkgResolutionId, ProjectId, ProjectRootDir } from '@pnpm/types'

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

test('updateRequested bypasses preferred-version propagation along the dep chain so a deeper caret consumer reaches latest (form-data regression)', async () => {
  // Regression test for the form-data downgrade (see PR pnpm/pnpm#12558):
  // when `pnpm up -r <pkg>` targets a transitive package, the deps-resolver
  // must compute `updateRequested: true` for that package's resolutions
  // and thread it through to `storeController.requestPackage`. The npm
  // picker then sets `preferredVersionSelectors: undefined` (verified at
  // the resolver-unit level in `resolving/npm-resolver/test/index.ts`)
  // and reaches `latest` instead of honoring an older sibling's
  // propagated preferred version.
  //
  // Topology that triggers the bug deterministically: a single branch
  // where the targeted package appears at two depths — a shallower
  // exact-pin (nx → form-data@4.0.5 role) and a deeper caret consumer
  // (axios → form-data@^4.0.5 role, but reached via a sub-carrier so
  // it resolves AFTER the exact-pin has propagated preferred down the
  // chain). Without the fix's `updateRequested` plumbing, the deeper
  // caret re-resolution honors the propagated preferred (1.0.0) over
  // `latest` (1.0.1) and the lockfile never bumps — the form-data bug.
  //
  // Cross-branch propagation does NOT happen in pnpm's preferredVersions
  // model (each branch builds its own `newPreferredVersions` chain via
  // `Object.create(preferredVersions)` at resolveDependencies.ts:754),
  // so a flat two-carrier topology can't reproduce the bug. The
  // propagation only flows down a single chain, which is why the
  // exact-pin must be an ANCESTOR-side sibling of the caret consumer.
  const requestLog: Array<{
    alias: string | undefined
    bareSpecifier: string | undefined
    updateRequested: boolean | undefined
    hadPreferredT100: boolean
    resolvedId: string
  }> = []
  const storeController = createStoreController(async (wantedDependency, options) => {
    const alias = wantedDependency.alias!
    const bareSpecifier = wantedDependency.bareSpecifier!
    const version = pickVersion(alias, bareSpecifier, options.preferredVersions, options.updateRequested)
    const resolvedId = `${alias}@${version}`
    requestLog.push({
      alias,
      bareSpecifier,
      updateRequested: options.updateRequested,
      // Snapshot the boolean at call time. `options.preferredVersions`
      // is a live object whose later mutations could otherwise make the
      // sanity assertion below pass even if the value was absent here.
      hadPreferredT100: Boolean(options.preferredVersions.t?.['1.0.0']),
      resolvedId,
    })
    return createPackageResponse(resolvedId)
  })
  const lockfile = createLockfileWithTPinning()

  await resolveDependencyTree([
    {
      id: '.' as ProjectId,
      manifest: {
        name: 'root',
        version: '0.0.0',
        dependencies: {
          // Carrier whose own manifest pins t at exactly 1.0.0
          // (nx → form-data@4.0.5 role) AND pulls in `inner`, whose
          // own manifest consumes t via caret (axios → form-data@^4.0.5
          // role). Nesting both under one carrier makes the exact-pin
          // a depth-1 sibling of `inner`, so its resolved 1.0.0
          // propagates down the chain to `inner`'s depth-2 caret
          // re-resolution via the preferredVersions chain built at
          // resolveDependencies.ts:754-770.
          multi: '1.0.0',
        },
      },
      modulesDir: '/project/node_modules',
      rootDir: '/project' as ProjectRootDir,
      updatePackageManifest: false,
      // Mirror `pnpm up -r t`: target t by name. The deps-resolver
      // invokes this for every package considered for re-resolution
      // (see `resolveDependencies.ts:895-899`) and threads the result
      // through as `updateRequested` to requestPackage.
      updateMatching: (name: string) => name === 't',
      wantedDependencies: [
        {
          alias: 'multi',
          bareSpecifier: '1.0.0',
          dev: false,
          optional: false,
          // Mirror `pnpm up -r`'s default depth (Infinity) so the
          // targeted update propagates through transitives —
          // `updateShouldContinue` at line 891 is a `currentDepth ≤
          // updateDepth` check.
          updateDepth: Number.POSITIVE_INFINITY,
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

  const tResolutions = requestLog.filter(({ alias }) => alias === 't')
  // Two resolutions of t reach requestPackage: multi's depth-1 exact
  // pin and inner's depth-2 caret consumer.
  expect(tResolutions).toHaveLength(2)

  const caretResolution = tResolutions.find(({ bareSpecifier }) => bareSpecifier === '^1.0.0')
  const exactResolution = tResolutions.find(({ bareSpecifier }) => bareSpecifier === '1.0.0')

  // Primary contract: the caret consumer (inner's transitive t@^1.0.0)
  // reaches `latest` (1.0.1), not the older version propagated down the
  // chain from multi's exact-pin (1.0.0). A `t@1.0.0` here is the
  // form-data downgrade — the targeted update quietly pinned the
  // caret consumer to the propagated preferred instead of letting it
  // bump.
  expect(caretResolution?.resolvedId).toBe('t@1.0.1')

  // Sanity check that the bug scenario was actually set up: the
  // caret consumer's preferredVersions must contain t/1.0.0
  // (propagated from multi's exact-pin). If this fails, the topology
  // no longer reproduces the bug and the primary assertion above is
  // meaningless.
  expect(caretResolution?.hadPreferredT100).toBe(true)

  // Companion outcome: the exact-pin resolves to 1.0.0 (range admits
  // nothing else).
  expect(exactResolution?.resolvedId).toBe('t@1.0.0')

  // Plumbing diagnostics: the targeted package's resolutions must
  // carry `updateRequested: true` so the npm picker bypasses
  // preferredVersionSelectors. The carriers (re-resolved because of
  // the broad `update` flag, but NOT the user's target) must carry
  // `updateRequested: false` — this per-package discrimination is the
  // core distinction the fix introduces over the broad `update` flag.
  for (const resolution of tResolutions) {
    expect(resolution.updateRequested).toBe(true)
  }
  // Assert every carrier resolution (not just the first) stays
  // `updateRequested: false`, so a stray re-resolution can't slip the
  // broad `update` flag through for a non-targeted package.
  const multiResolutions = requestLog.filter(({ alias }) => alias === 'multi')
  expect(multiResolutions.length).toBeGreaterThan(0)
  for (const resolution of multiResolutions) {
    expect(resolution.updateRequested).toBe(false)
  }
  const innerResolutions = requestLog.filter(({ alias }) => alias === 'inner')
  expect(innerResolutions.length).toBeGreaterThan(0)
  for (const resolution of innerResolutions) {
    expect(resolution.updateRequested).toBe(false)
  }
})

function hasPreferredVersion (preferredVersions: PreferredVersions, alias: string, version: string): boolean {
  return Boolean(preferredVersions[alias]?.[version])
}

function pickVersion (alias: string, bareSpecifier: string, preferredVersions: PreferredVersions, updateRequested?: boolean): string {
  if (alias === 'provider' && bareSpecifier === '*') {
    return hasPreferredVersion(preferredVersions, alias, '1.0.0') ? '1.0.0' : '2.0.0'
  }
  if (alias === 't') {
    // An exact specifier admits only itself; the updateRequested bypass
    // cannot widen an exact pin.
    if (bareSpecifier === '1.0.0') return '1.0.0'
    // Caret admits 1.0.0 and 1.0.1 (latest). Model the npm-picker
    // contract from `resolving/npm-resolver/src/index.ts`: when
    // `updateRequested` is true, `preferredVersionSelectors` is
    // undefined and the picker ignores propagated preferred versions.
    // Otherwise the picker honors a propagated 1.0.0 — the form-data
    // bug behavior the fix eliminates.
    if (bareSpecifier === '^1.0.0') {
      if (updateRequested === true) return '1.0.1'
      return hasPreferredVersion(preferredVersions, 't', '1.0.0') ? '1.0.0' : '1.0.1'
    }
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

/**
 * Mirror `pnpm up -r t`'s starting state. The prior lockfile records
 * `multi@1.0.0` (the carrier whose manifest pins t at 1.0.0 and pulls
 * in `inner`), `inner@1.0.0` (whose own manifest consumes t via caret
 * `^1.0.0`), and `t@1.0.0` (the version both branches resolved to
 * under the older `latest` dist-tag). `infoFromLockfile` is only
 * populated when the package is recorded here, which the deps-resolver
 * needs to even invoke `updateMatching` (see
 * `resolveDependencies.ts:895-899`).
 */
function createLockfileWithTPinning (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: {
          multi: '1.0.0',
        },
        dependencies: {
          multi: '1.0.0',
        },
      },
    },
    packages: {
      ['multi@1.0.0' as DepPath]: {
        id: 'multi@1.0.0',
        name: 'multi',
        version: '1.0.0',
        resolution: { type: 'directory', directory: '/dev/null' },
        dependencies: { t: '1.0.0', inner: '1.0.0' },
      },
      ['inner@1.0.0' as DepPath]: {
        id: 'inner@1.0.0',
        name: 'inner',
        version: '1.0.0',
        resolution: { type: 'directory', directory: '/dev/null' },
        dependencies: { t: '1.0.0' },
      },
      ['t@1.0.0' as DepPath]: {
        id: 't@1.0.0',
        name: 't',
        version: '1.0.0',
        resolution: { type: 'directory', directory: '/dev/null' },
      },
    },
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
  't@1.0.0': {
    name: 't',
    version: '1.0.0',
  },
  't@1.0.1': {
    name: 't',
    version: '1.0.1',
  },
  'multi@1.0.0': {
    name: 'multi',
    version: '1.0.0',
    dependencies: {
      // Shallower exact pin on t (nx → form-data@4.0.5 role).
      t: '1.0.0',
      // Sub-carrier whose own manifest consumes t via caret (axios →
      // form-data@^4.0.5 role).
      inner: '1.0.0',
    },
  },
  'inner@1.0.0': {
    name: 'inner',
    version: '1.0.0',
    dependencies: {
      // Caret consumer — admits 1.0.0 (initial) and 1.0.1 (after the
      // dist-tag bump that `pnpm up -r t` reacts to).
      t: '^1.0.0',
    },
  },
}
