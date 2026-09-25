import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import {
  type FastSettingsUpdateOptions,
  tryFastUpdateSettings,
} from '../../src/install/tryFastUpdateSettings.js'
import { testDefaults } from '../utils/index.js'

const NEW_SETTINGS = {
  autoInstallPeers: false,
  dedupePeers: true,
  excludeLinksFromLockfile: true,
  peersSuffixMaxLength: 10,
  injectWorkspacePackages: true,
}

test.each([
  'settings.autoInstallPeers',
  'settings.dedupePeers',
  'settings.excludeLinksFromLockfile',
  'settings.peersSuffixMaxLength',
  'settings.injectWorkspacePackages',
] as const)('%s is recorded without resolution when the lockfile has no dependency it could affect', (changedSetting) => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: [changedSetting],
  }))).toBe(true)
  expect(lockfile.settings).toStrictEqual(NEW_SETTINGS)
})

test.each([
  'settings.autoInstallPeers',
  'settings.dedupePeers',
  'settings.peersSuffixMaxLength',
] as const)('%s falls back when a package declares peer dependencies', (changedSetting) => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.packages!['foo@1.0.0' as keyof typeof lockfile.packages] = {
    peerDependencies: { bar: '^1.0.0' },
  } as never

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: [changedSetting],
  }))).toBe(false)
  expect(lockfile.settings).toBeUndefined()
})

test('a peer setting falls back when a dep path carries a peers suffix', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.packages!['foo@1.0.0(bar@1.0.0)' as keyof typeof lockfile.packages] = {} as never

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.dedupePeers'],
  }))).toBe(false)
})

test('a peer setting falls back when a project declares peer dependencies', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.autoInstallPeers'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: { foo: '^1.0.0' },
        peerDependencies: { bar: '^1.0.0' },
      },
    }],
  }))).toBe(false)
})

test('excludeLinksFromLockfile falls back when a project depends on a directory', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.excludeLinksFromLockfile'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: 'link:../bar',
          foo: '^1.0.0',
        },
      },
    }],
  }))).toBe(false)
})

test('excludeLinksFromLockfile falls back when a link is shadowed by the same alias in another group', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.excludeLinksFromLockfile'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: '^1.0.0',
          foo: '^1.0.0',
        },
        devDependencies: {
          bar: 'link:../bar',
        },
      },
    }],
  }))).toBe(false)
})

test('excludeLinksFromLockfile falls back when the lockfile records a link', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.importers['.' as ProjectId].dependencies!.bar = 'link:../bar'

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.excludeLinksFromLockfile'],
  }))).toBe(false)
})

test('excludeLinksFromLockfile is recorded when the only workspace dependency uses the workspace protocol', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.excludeLinksFromLockfile'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: 'workspace:*',
          foo: '^1.0.0',
        },
      },
    }],
    workspacePackages: new Map([['bar', new Map()]]) as WorkspacePackages,
  }))).toBe(true)
})

test('excludeLinksFromLockfile falls back when a workspace project is depended on by range', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.excludeLinksFromLockfile'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: '^1.0.0',
          foo: '^1.0.0',
        },
      },
    }],
    workspacePackages: new Map([['bar', new Map()]]) as WorkspacePackages,
  }))).toBe(false)
})

test.each([
  ['workspace:*', new Map([['bar', new Map()]]) as WorkspacePackages],
  ['^1.0.0', new Map([['bar', new Map()]]) as WorkspacePackages],
  ['link:../bar', new Map() as WorkspacePackages],
])('injectWorkspacePackages falls back when a workspace project is depended on as %s', (bareSpecifier, workspacePackages) => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.injectWorkspacePackages'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: bareSpecifier,
          foo: '^1.0.0',
        },
      },
    }],
    workspacePackages,
  }))).toBe(false)
})

test('injectWorkspacePackages falls back when a dependency is already injected', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.injectWorkspacePackages'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: { foo: '^1.0.0' },
        dependenciesMeta: { foo: { injected: true } },
      },
    }],
  }))).toBe(false)
})

test('a group of changed settings is recorded only when every one of them is safe', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdateSettings(lockfile, updateOptions({
    changedSettings: ['settings.dedupePeers', 'settings.injectWorkspacePackages'],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          bar: 'link:../bar',
          foo: '^1.0.0',
        },
      },
    }],
  }))).toBe(false)
  expect(lockfile.settings).toBeUndefined()
})

test('toggling dedupePeers on a peerless lockfile skips resolution', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.dedupePeers = true

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  expect(project.readLockfile().settings.dedupePeers).toBe(true)
})

test('toggling dedupePeers falls back to resolution when peer dependencies are locked', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/has-optional-peer-with-peer': '^1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.dedupePeers = true

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages.length).toBeGreaterThan(0)
})

function updateOptions (opts: Partial<FastSettingsUpdateOptions>): FastSettingsUpdateOptions {
  return {
    changedSettings: [],
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: { foo: '^1.0.0' },
      },
    }],
    settings: NEW_SETTINGS,
    workspacePackages: new Map() as WorkspacePackages,
    ...opts,
  }
}

function lockfileWithRegistryDependency (): LockfileObject {
  return {
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '^1.0.0',
        },
      },
    },
    lockfileVersion: '9.0',
    packages: {
      'foo@1.0.0': {},
    },
  } as unknown as LockfileObject
}

function trackRequestedPackages (storeController: StoreController): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}
