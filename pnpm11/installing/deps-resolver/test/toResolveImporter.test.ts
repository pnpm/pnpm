import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'

import type { ImporterToResolve } from '../lib/index.js'
import { toResolveImporter } from '../lib/toResolveImporter.js'

test('a direct dependency named after an Object.prototype property gets preferred versions of its own', async () => {
  const rootDir = path.join(os.tmpdir(), `pnpm-to-resolve-importer-${process.pid}`) as ProjectRootDir
  const project: ImporterToResolve = {
    binsDir: path.join(rootDir, 'node_modules/.bin'),
    id: '.' as ProjectId,
    manifest: {
      dependencies: {
        constructor: '1.0.0',
      },
    },
    modulesDir: path.join(rootDir, 'node_modules'),
    rootDir,
    updatePackageManifest: false,
    wantedDependencies: [],
  }
  const importer = await toResolveImporter({
    defaultUpdateDepth: 0,
    globalVirtualStoreDir: path.join(rootDir, 'node_modules/.pnpm'),
    hideAlienModules: false,
    noDependencySelectors: true,
    virtualStoreDir: path.join(rootDir, 'node_modules/.pnpm'),
    workspacePackages: new Map(),
  }, project)

  expect(Object.hasOwn(importer.preferredVersions!, 'constructor')).toBe(true)
  expect(importer.preferredVersions!['constructor']).toStrictEqual({ '1.0.0': 'version' })
  expect(Object.hasOwn(Object, '1.0.0')).toBe(false)
})
