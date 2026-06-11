import { expect, test } from '@jest/globals'

import type { DependenciesGraph } from '../lib/buildSequence.js'
import { buildModules } from '../lib/index.js'

const baseOpts = {
  depsStateCache: {},
  lockfileDir: '/project',
  optional: true,
  rootModulesDir: '/project/node_modules',
  sideEffectsCacheWrite: false,
  storeController: {} as never,
  unsafePerm: false,
  userAgent: 'pnpm',
}

interface NodeOverrides {
  requiresBuild?: boolean
  patch?: object
  isBuilt?: boolean
  optional?: boolean
}

function singlePkgGraph (depPath: string, overrides: NodeOverrides): DependenciesGraph<string> {
  return {
    [depPath]: {
      children: {},
      depPath,
      name: 'foo',
      version: '1.0.0',
      dir: '/store/links/hash/node_modules/foo',
      hasBin: false,
      hasBundledDependencies: false,
      optional: false,
      optionalDependencies: new Set<string>(),
      ...overrides,
    },
  } as unknown as DependenciesGraph<string>
}

const allowFoo = (depPath: string): boolean => depPath === 'foo@1.0.0'

test('frozenStore + GVS: an approved build that is not cached refuses up front', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: allowFoo,
      enableGlobalVirtualStore: true,
      frozenStore: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_STORE_NEEDS_BUILD',
  })
})

test('frozenStore + GVS: a patched package that is not cached refuses up front', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { patch: { hash: 'h', path: '/p' }, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      enableGlobalVirtualStore: true,
      frozenStore: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_STORE_NEEDS_BUILD',
  })
})

test('frozenStore + GVS: an already-built (cached) package does not trip the backstop', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: true }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: allowFoo,
      enableGlobalVirtualStore: true,
      frozenStore: true,
    })
  ).resolves.toBeDefined()
})

test('frozenStore + GVS: a build-requiring package that is not approved does not trip the backstop', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: () => false,
      enableGlobalVirtualStore: true,
      frozenStore: true,
      ignoreScripts: true,
    })
  ).resolves.toBeDefined()
})

test('frozenStore + GVS: an approved build under ignoreScripts is not blocked (the script never runs)', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: allowFoo,
      enableGlobalVirtualStore: true,
      frozenStore: true,
      ignoreScripts: true,
    })
  ).resolves.toBeDefined()
})

test('frozenStore + GVS: a patched package under ignoreScripts still refuses (the patch is applied regardless)', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { patch: { hash: 'h', path: '/p' }, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      enableGlobalVirtualStore: true,
      frozenStore: true,
      ignoreScripts: true,
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_STORE_NEEDS_BUILD',
  })
})

test('frozenStore + GVS: an optional approved build that is not cached is skipped, not blocked', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: false, optional: true }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: allowFoo,
      enableGlobalVirtualStore: true,
      frozenStore: true,
    })
  ).resolves.toBeDefined()
})

test('frozenStore + GVS: an optional patched package that is not cached is skipped, not blocked', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { patch: { hash: 'h', path: '/p' }, isBuilt: false, optional: true }), ['foo@1.0.0'], {
      ...baseOpts,
      enableGlobalVirtualStore: true,
      frozenStore: true,
    })
  ).resolves.toBeDefined()
})

test('frozenStore without GVS: an approved build is not blocked (builds write to the writable project store)', async () => {
  await expect(
    buildModules(singlePkgGraph('foo@1.0.0', { requiresBuild: true, isBuilt: false }), ['foo@1.0.0'], {
      ...baseOpts,
      allowBuild: allowFoo,
      enableGlobalVirtualStore: false,
      frozenStore: true,
      ignoreScripts: true,
    })
  ).resolves.toBeDefined()
})
