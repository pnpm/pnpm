import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'

import { wantedDepIsLocallyAvailable } from '../lib/wantedDepIsLocallyAvailable.js'

test('a workspace package whose version has build metadata satisfies the exact version', () => {
  const workspacePackages = new Map([
    ['foo', new Map([
      ['1.0.0-next.3+f60facc', {
        rootDir: '/repo/foo' as ProjectRootDir,
        manifest: { name: 'foo', version: '1.0.0-next.3+f60facc' },
      }],
    ])],
  ])
  expect(wantedDepIsLocallyAvailable(
    workspacePackages,
    { alias: 'foo', bareSpecifier: '1.0.0-next.3+f60facc', dev: false, optional: false },
    { defaultTag: 'latest', registry: 'https://registry.npmjs.org/' }
  )).toBe(true)
})
