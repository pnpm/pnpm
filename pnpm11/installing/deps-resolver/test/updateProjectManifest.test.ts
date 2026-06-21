import { expect, test } from '@jest/globals'
import type { PkgResolutionId, ProjectId, ProjectRootDir } from '@pnpm/types'

import type { ImporterToResolve } from '../lib/index.js'
import type { ResolvedDirectDependency } from '../lib/resolveDependencyTree.js'
import { updateProjectManifest } from '../lib/updateProjectManifest.js'

test('updateProjectManifest preserves workspace protocol specs when requested', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:../packages/foo/dist'), {
    directDependencies: [createDirectDependency()],
    preserveWorkspaceProtocol: true,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('workspace:../packages/foo/dist')
})

test('updateProjectManifest saves normalized local specs when workspace protocol is not preserved', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:../packages/foo/dist'), {
    directDependencies: [createDirectDependency()],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: 'rolling',
  })

  expect(manifest?.dependencies?.foo).toBe('link:../packages/foo/dist')
})

test('updateProjectManifest saves normalized workspace range specs', async () => {
  const [manifest] = await updateProjectManifest(createImporter('workspace:*'), {
    directDependencies: [
      createDirectDependency({
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
      createDirectDependency({
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
      {
        alias: 'react-dom',
        bareSpecifier: 'foo',
        dev: false,
        optional: true,
        updateSpec: true,
      },
      {
        alias: 'react',
        bareSpecifier: '19.0.0',
        dev: true,
        optional: false,
      },
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
      } as ResolvedDirectDependency,
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
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [
      {
        bareSpecifier: 'pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
        dev: false,
        optional: false,
        updateSpec: true,
      },
    ],
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

test('updates manifest for GitHub URL dependencies without aliases', async () => {
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [
      {
        bareSpecifier: 'https://github.com/pnpm/test-git-fetch.git#8b333f12d5357f4f25a654c305c826294cb073bf',
        dev: false,
        optional: false,
        updateSpec: true,
      },
    ],
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
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    updatePackageManifest: true,
    wantedDependencies: [
      {
        bareSpecifier: 'jsr:@foo/bar',
        dev: false,
        optional: false,
        updateSpec: true,
      },
    ],
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

test('pairs multiple aliasless dependencies with their resolutions in order', async () => {
  const [manifest] = await updateProjectManifest({
    binsDir: '/project/node_modules/.bin',
    id: '.' as ProjectId,
    manifest: {},
    modulesDir: '/project/node_modules',
    rootDir: '/project' as ProjectRootDir,
    targetDependenciesField: 'dependencies',
    updatePackageManifest: true,
    wantedDependencies: [
      {
        alias: 'foo',
        bareSpecifier: '^1.0.0',
        dev: false,
        optional: false,
        updateSpec: true,
      },
      {
        bareSpecifier: 'jsr:@rus/greet@0.0.3',
        dev: false,
        optional: false,
        updateSpec: true,
      },
      {
        bareSpecifier: 'github:kevva/is-positive#97edff6',
        dev: false,
        optional: false,
        updateSpec: true,
      },
    ],
  } as ImporterToResolve, {
    directDependencies: [
      {
        alias: 'foo',
        name: 'foo',
        normalizedBareSpecifier: '^1.0.0',
        pkgId: 'foo@1.0.0',
        resolution: {},
        version: '1.0.0',
      },
      {
        alias: '@rus/greet',
        name: '@rus/greet',
        normalizedBareSpecifier: 'jsr:^0.0.3',
        pkgId: '@rus/greet@0.0.3',
        resolution: {},
        version: '0.0.3',
      },
      {
        alias: 'is-positive',
        name: 'is-positive',
        normalizedBareSpecifier: 'github:kevva/is-positive#97edff6',
        pkgId: 'is-positive@github:kevva/is-positive#97edff6',
        resolution: {},
        version: undefined,
      },
    ] as unknown as ResolvedDirectDependency[],
    preserveWorkspaceProtocol: false,
    saveWorkspaceProtocol: false,
  })

  expect(manifest).toStrictEqual({
    dependencies: {
      foo: '^1.0.0',
      '@rus/greet': 'jsr:^0.0.3',
      'is-positive': 'github:kevva/is-positive#97edff6',
    },
  })
})

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
    wantedDependencies: [
      {
        alias: 'foo',
        bareSpecifier,
        dev: false,
        optional: false,
        updateSpec: true,
      },
    ],
  }
}

function createDirectDependency (overrides: Partial<ResolvedDirectDependency> = {}): ResolvedDirectDependency {
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
    ...overrides,
  }
}
