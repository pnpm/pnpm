import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'

import { getDepsToResolve } from '../lib/getDepsToResolve.js'

const NIGHTLY = '25.0.0-nightly20250101abcdef'

function lockfileWithRuntime (version: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      [`node@runtime:${version}` as never]: {
        resolution: { type: 'variations', variants: [] },
        version,
      },
    },
  } as unknown as LockfileObject
}

function resolveLockedRuntime (bareSpecifier: string, locked: 'resolvedDependencies' | 'lockedDependencies') {
  const [resolved] = getDepsToResolve([{ alias: 'node', bareSpecifier, dev: false, optional: false }], lockfileWithRuntime(NIGHTLY), {
    registriesByScope: { default: 'https://registry.example.test/' },
    currentDepth: 1,
    prefix: '/project',
    proceed: false,
    [locked]: { node: `runtime:${NIGHTLY}` },
  })
  return resolved.infoFromLockfile?.depPath
}

test('a locked runtime is reused while the range of its selector admits it', () => {
  for (const locked of ['resolvedDependencies', 'lockedDependencies'] as const) {
    expect(resolveLockedRuntime('runtime:nightly/^25', locked)).toBe(`node@runtime:${NIGHTLY}`)
    expect(resolveLockedRuntime('runtime:nightly/^26', locked)).toBeUndefined()
  }
})
