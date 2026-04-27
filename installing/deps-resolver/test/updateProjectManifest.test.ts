import { expect, test } from '@jest/globals'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'

import type { ImporterToResolve } from '../lib/index.js'
import type { ResolvedDirectDependency } from '../lib/resolveDependencyTree.js'
import { updateProjectManifest } from '../lib/updateProjectManifest.js'

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
      } as ResolvedDirectDependency,
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
