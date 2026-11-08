import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'

import { getWorkspacePackagesByDirectory } from '../src/getWorkspacePackagesByDirectory.js'

test('getWorkspacePackagesByDirectory(): a publish directory at another project root keeps that project manifest', () => {
  const aManifest = { name: 'a', version: '1.0.0', publishConfig: { directory: '../b' } }
  const bManifest = { name: 'b', version: '2.0.0' }
  const byDirectory = getWorkspacePackagesByDirectory(new Map([
    ['b', new Map([['2.0.0', { rootDir: 'ws/b' as ProjectRootDir, manifest: bManifest }]])],
    ['a', new Map([['1.0.0', { rootDir: 'ws/a' as ProjectRootDir, manifest: aManifest }]])],
  ]))
  expect(byDirectory['ws/b']).toBe(bManifest)
})
