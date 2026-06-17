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
