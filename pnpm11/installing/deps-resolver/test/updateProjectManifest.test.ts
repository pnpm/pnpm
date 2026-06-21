import { expect, test } from '@jest/globals'
import type { PkgResolutionId, ProjectId, ProjectRootDir } from '@pnpm/types'

import type { WantedDependency } from '../lib/getNonDevWantedDependencies.js'
import type { ImporterToResolve } from '../lib/index.js'
import type { ResolvedDirectDependency } from '../lib/resolveDependencyTree.js'
import { updateProjectManifest } from '../lib/updateProjectManifest.js'

test('updateProjectManifest preserves workspace protocol specs when requested', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:../packages/foo/dist'), {
    directDependencies: [createDirectDependency('workspace:../packages/foo/dist')],
    preserveWorkspaceProtocol: true,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('workspace:../packages/foo/dist')
})

test('updateProjectManifest saves normalized local specs when workspace protocol is not preserved', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:../packages/foo/dist'), {
    directDependencies: [createDirectDependency('workspace:../packages/foo/dist')],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('link:../packages/foo/dist')
})

test('updateProjectManifest saves normalized workspace range specs', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:*'), {
    directDependencies: [
      createDirectDependency('workspace:*', {
        normalizedBareSpecifier: 'workspace:^1.0.0',
      }),
    ],
    preserveWorkspaceProtocol: true,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('workspace:^1.0.0')
})

test('updateProjectManifest preserves catalog specifier precedence', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:../packages/foo/dist'), {
    directDependencies: [
      createDirectDependency('workspace:../packages/foo/dist', {
        catalogLookup: {
          catalogName: 'default',
          specifier: '^1.0.0',
          userSpecifiedBareSpecifier: 'catalog:',
        },
      }),
    ],
    preserveWorkspaceProtocol: true,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('catalog:')
})

test('does not update an unrelated dependency when an optional dependency update fails to resolve', async () => {
  // `react-dom` is the failed optional update, so it is absent from
  // `directDependencies`.
  const reactWanted: WantedDependency = { alias: 'react', bareSpecifier: '19.0.0', dev: true, optional: false }
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {
      devDependencies: {
        react: '19.0.0',
      },
      optionalDependencies: {
        'react-dom': '19.0.0',
      },
    },
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [
      { alias: 'react-dom', bareSpecifier: 'foo', dev: false, optional: true, updateSpec: true },
      reactWanted,
    ],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: 'react',
        dev: true,
        name: 'react',
        optional: false,
        pkgId: 'react@19.0.0',
        resolution: {},
        version: '19.0.0',
        wantedDependency: reactWanted,
      } as unknown as ResolvedDirectDependency,
    ],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    devDependencies: {
      react: '19.0.0',
    },
    optionalDependencies: {
      'react-dom': '19.0.0',
    },
  })
})

test('updates manifest for GitHub shorthand dependencies without aliases', async () => {
  const wantedDependency = aliaslessWantedDependency('pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf')
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [wantedDependency],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: 'test-git-fetch',
        dev: false,
        name: 'test-git-fetch',
        normalizedBareSpecifier: 'github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
        optional: false,
        pkgId: 'test-git-fetch@github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
        resolution: {},
        version: undefined,
        wantedDependency,
      } as unknown as ResolvedDirectDependency,
    ],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    dependencies: {
      'test-git-fetch': 'github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
    },
  })
})

test('updates manifest for aliasless dependencies whose specifier does not resemble the resolution (jsr)', async () => {
  const wantedDependency = aliaslessWantedDependency('jsr:@foo/bar')
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [wantedDependency],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: '@foo/bar',
        dev: false,
        name: '@foo/bar',
        normalizedBareSpecifier: 'jsr:^0.1.0',
        optional: false,
        pkgId: '@foo/bar@0.1.0',
        resolution: {},
        version: '0.1.0',
        wantedDependency,
      } as unknown as ResolvedDirectDependency,
    ],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    dependencies: {
      '@foo/bar': 'jsr:^0.1.0',
    },
  })
})

test('updates an aliasless selector that resolves to an alias already present in the manifest', async () => {
  // The resolved alias collides with the existing (non-updating) manifest
  // entry; the resolution carries the new selector, so its spec wins.
  const newSelector = aliaslessWantedDependency('pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf')
  const existingEntry: WantedDependency = {
    alias: 'test-git-fetch',
    bareSpecifier: 'github:pnpm/test-git-fetch#0000000000000000000000000000000000000000',
    dev: false,
    optional: false,
  }
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {
      dependencies: {
        'test-git-fetch': 'github:pnpm/test-git-fetch#0000000000000000000000000000000000000000',
      },
    },
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [existingEntry, newSelector],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: 'test-git-fetch',
        dev: false,
        name: 'test-git-fetch',
        normalizedBareSpecifier: 'github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
        optional: false,
        pkgId: 'test-git-fetch@github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
        resolution: {},
        version: undefined,
        wantedDependency: newSelector,
      } as unknown as ResolvedDirectDependency,
    ],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    dependencies: {
      'test-git-fetch': 'github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
    },
  })
})

test('does not misattribute a spec when an aliasless optional dependency fails to resolve', async () => {
  // The survivor omits `normalizedBareSpecifier` so its spec falls back to the
  // wanted dependency's — the path where a wrong pairing would surface.
  const failedOptional = aliaslessWantedDependency('github:owner/missing#1111111111111111111111111111111111111111', true)
  const survivor = aliaslessWantedDependency('github:owner/good#2222222222222222222222222222222222222222')
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [failedOptional, survivor],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: 'good',
        dev: false,
        name: 'good',
        normalizedBareSpecifier: undefined,
        optional: false,
        pkgId: 'good@github:owner/good#2222222222222222222222222222222222222222',
        resolution: {},
        version: undefined,
        wantedDependency: survivor,
      } as unknown as ResolvedDirectDependency,
    ],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    dependencies: {
      good: 'github:owner/good#2222222222222222222222222222222222222222',
    },
  })
})

// Aliasless selectors (`jsr:@x/y`, a bare `owner/repo#sha`, a GitHub URL) carry
// no alias at the parse seam, where `parseWantedDependencies` casts them to
// `WantedDependency[]` despite the interface's `alias: string`. Mirror that one
// cast here instead of repeating it at every fixture.
function aliaslessWantedDependency (bareSpecifier: string, optional = false): WantedDependency {
  return { bareSpecifier, dev: false, optional, updateSpec: true } as unknown as WantedDependency
}

function createImporter (bareSpecifier: string): ImporterToResolve {
  return {
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {
      dependencies: {
        foo: bareSpecifier,
      },
    },
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    targetDependenciesField: 'dependencies',
    updatePackageManifest: true,
    wantedDependencies: [fooWantedDependency(bareSpecifier)],
  }
}

function fooWantedDependency (bareSpecifier: string): WantedDependency {
  return { alias: 'foo', bareSpecifier, dev: false, optional: false, updateSpec: true }
}

function createDirectDependency (bareSpecifier: string, overrides: Partial<ResolvedDirectDependency> = {}): ResolvedDirectDependency {
  return {
    alias: 'foo',
    dev: false,
    name: 'foo',
    normalizedBareSpecifier: 'link:../packages/foo/dist',
    optional: false,
    pkgId: 'link:../packages/foo/dist' as PkgResolutionId,
    resolution: {
      directory: '../packages/foo/dist',
      type: 'directory',
    },
    version: '1.0.0',
    wantedDependency: fooWantedDependency(bareSpecifier),
    ...overrides,
  }
}
