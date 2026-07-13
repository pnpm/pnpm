import { describe, expect, test } from '@jest/globals'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

import { buildLockfileFromEnvLockfile } from '../../src/self-updater/installPnpm.js'

describe('buildLockfileFromEnvLockfile', () => {
  test('falls back to base package entry for peer-suffixed package', () => {
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

    expect(result.packages['fdir@6.5.0(picomatch@4.0.5)' as DepPath]).toEqual({
      resolution: { integrity: 'sha512-base' },
      dependencies: { picomatch: '4.0.5' },
    })

    expect(result.packages['other@1.0.0' as DepPath]).toEqual({
      resolution: { integrity: 'sha512-other' },
      dependencies: {},
    })
  })
})
