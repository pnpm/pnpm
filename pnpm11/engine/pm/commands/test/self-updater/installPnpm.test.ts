import { describe, expect, test } from '@jest/globals'
import { buildLockfileFromEnvLockfile } from '@pnpm/engine.pm.commands'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

describe('buildLockfileFromEnvLockfile', () => {
  test('reads the resolution of a peer-suffixed snapshot from its base package entry', () => {
    const envLockfile = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'fdir@6.5.0': {
          resolution: { integrity: 'sha512-base' },
        },
        'other@1.0.0': {
          resolution: { integrity: 'sha512-other' },
        },
      },
      snapshots: {
        'fdir@6.5.0(picomatch@4.0.5)': {
          dependencies: { picomatch: '4.0.5' },
        },
        'other@1.0.0': {
          dependencies: {},
        },
      },
    } as unknown as EnvLockfile

    const result = buildLockfileFromEnvLockfile(envLockfile, 'pnpm', '11.12.0')

    expect(result.packages!['fdir@6.5.0(picomatch@4.0.5)' as DepPath]).toEqual({
      resolution: { integrity: 'sha512-base' },
      dependencies: { picomatch: '4.0.5' },
    })

    expect(result.packages!['other@1.0.0' as DepPath]).toEqual({
      resolution: { integrity: 'sha512-other' },
      dependencies: {},
    })
  })

  test('keeps the patch hash when stripping the peers suffix', () => {
    const envLockfile = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'foo@1.0.0(patch_hash=abc)': {
          resolution: { integrity: 'sha512-patched' },
        },
      },
      snapshots: {
        'foo@1.0.0(patch_hash=abc)(react@17.0.0)': {
          dependencies: { react: '17.0.0' },
        },
      },
    } as unknown as EnvLockfile

    const result = buildLockfileFromEnvLockfile(envLockfile, 'pnpm', '11.12.0')

    expect(result.packages!['foo@1.0.0(patch_hash=abc)(react@17.0.0)' as DepPath]).toEqual({
      resolution: { integrity: 'sha512-patched' },
      dependencies: { react: '17.0.0' },
    })
  })
})
