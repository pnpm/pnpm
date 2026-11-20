import { expect, test } from '@jest/globals'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { ProjectRootDir } from '@pnpm/types'

import { wantedDepIsLocallyAvailable } from '../src/wantedDepIsLocallyAvailable.js'

test('wantedDepIsLocallyAvailable matches packages with build metadata', () => {
  const workspacePackages: WorkspacePackages = new Map([
    ['foo', new Map([
      ['0.5.6-next.3+f60facc', {
        rootDir: '/workspace/foo' as ProjectRootDir,
        manifest: {
          name: 'foo',
          version: '0.5.6-next.3+f60facc',
        },
      }],
    ])],
  ])

  for (const bareSpecifier of [
    '0.5.6-next.3+f60facc',
    '0.5.6-next.3',
    '^0.5.6-next.3+f60facc',
    '^0.5.6-next.3',
    '~0.5.6-next.3+f60facc',
    '*',
  ]) {
    expect(wantedDepIsLocallyAvailable(
      workspacePackages,
      { alias: 'foo', bareSpecifier, dev: false, optional: false },
      { defaultTag: 'latest', registry: 'https://registry.npmjs.org/' }
    )).toBe(true)
  }
})

test('wantedDepIsLocallyAvailable preserves stable-only behavior for tag selectors', () => {
  const workspacePackages: WorkspacePackages = new Map([
    ['foo', new Map([
      ['0.5.6-next.3+f60facc', {
        rootDir: '/workspace/foo' as ProjectRootDir,
        manifest: {
          name: 'foo',
          version: '0.5.6-next.3+f60facc',
        },
      }],
    ])],
    ['bar', new Map([
      ['1.0.0', {
        rootDir: '/workspace/bar' as ProjectRootDir,
        manifest: {
          name: 'bar',
          version: '1.0.0',
        },
      }],
    ])],
  ])

  expect(wantedDepIsLocallyAvailable(
    workspacePackages,
    { alias: 'foo', bareSpecifier: 'latest', dev: false, optional: false },
    { defaultTag: 'latest', registry: 'https://registry.npmjs.org/' }
  )).toBe(false)

  expect(wantedDepIsLocallyAvailable(
    workspacePackages,
    { alias: 'bar', bareSpecifier: 'latest', dev: false, optional: false },
    { defaultTag: 'latest', registry: 'https://registry.npmjs.org/' }
  )).toBe(true)
})

