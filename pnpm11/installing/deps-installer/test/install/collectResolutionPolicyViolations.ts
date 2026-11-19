import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import {
  collectResolutionPolicyViolations,
  RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
  verifyLockfileResolutions,
  withOfflineCheckCacheIdentities,
} from '../../src/install/verifyLockfileResolutions.js'
import { recordVerification } from '../../src/install/verifyLockfileResolutionsCache.js'

const NOOP_SLOT = {
  policy: {} as Record<string, unknown>,
  canTrustPastCheck: () => true,
}

function makeLockfile (packages: Record<string, { resolution: unknown }>): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {},
    packages: packages as LockfileObject['packages'],
  } as LockfileObject
}

test.each([
  ['file-tarball', { integrity: 'sha512-deadbeef', tarball: 'file:alt.tgz' }],
  ['directory', { type: 'directory', directory: '../alt' }],
  ['git', { type: 'git', repo: 'https://example.com/safe-pkg.git', commit: '0123456789abcdef0123456789abcdef01234567' }],
] as const)('collectResolutionPolicyViolations reports RESOLUTION_SHAPE_MISMATCH for %s', async (_label, resolution) => {
  const lockfile = makeLockfile({
    'safe-pkg@1.0.0': { resolution },
  })
  let verifierCalls = 0
  const verifier: ResolutionVerifier = {
    ...NOOP_SLOT,
    verify: async () => {
      verifierCalls++
      return { ok: true }
    },
  }

  for (const verifiers of [[], [verifier]]) {
    // eslint-disable-next-line no-await-in-loop -- test both empty and active verifier slices sequentially so verifierCalls stays deterministic
    const violations = await collectResolutionPolicyViolations(lockfile, verifiers)
    expect(violations).toEqual([
      expect.objectContaining({
        name: 'safe-pkg',
        version: '1.0.0',
        code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
      }),
    ])
  }
  expect(verifierCalls).toBe(0)
})

test('poisoned lockfile-verified.jsonl record cannot bypass resolution shape check', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-shape-cache-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, "lockfileVersion: '9.0'\n")
    const lockfile = makeLockfile({
      'safe-pkg@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: 'file:alt.tgz' } },
    })
    const verifier: ResolutionVerifier = {
      ...NOOP_SLOT,
      verify: async () => ({ ok: true }),
    }
    recordVerification(cacheDir, {
      lockfilePath,
      verifiers: withOfflineCheckCacheIdentities([verifier]),
      hashLockfile: () => hashObject(lockfile),
    })

    await expect(verifyLockfileResolutions(lockfile, [verifier], {
      cacheDir,
      lockfilePath,
    })).rejects.toMatchObject({
      code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    })
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})
