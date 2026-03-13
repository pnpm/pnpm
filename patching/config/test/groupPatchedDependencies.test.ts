import type { ExtendedPatchInfo, PatchGroupRecord } from '@pnpm/patching.types'

import { groupPatchedDependencies } from '../src/groupPatchedDependencies.js'

function sanitizePatchGroupRecord (patchGroups: PatchGroupRecord): PatchGroupRecord {
  for (const name in patchGroups) {
    patchGroups[name].range.sort((a, b) => a.version.localeCompare(b.version))
  }
  return patchGroups
}

const _groupPatchedDependencies: typeof groupPatchedDependencies = patchedDependencies => sanitizePatchGroupRecord(groupPatchedDependencies(patchedDependencies))

test('groups patchedDependencies according to names, match types, and versions', () => {
  const patchedDependencies: Record<string, string> = {
    'exact-version-only@0.0.0': '00000000000000000000000000000000',
    'exact-version-only@1.2.3': '00000000000000000000000000000000',
    'exact-version-only@2.1.0': '00000000000000000000000000000000',
    'version-range-only@~1.2.0': '00000000000000000000000000000000',
    'version-range-only@4': '00000000000000000000000000000000',
    'star-version-range@*': '00000000000000000000000000000000',
    'without-versions': '00000000000000000000000000000000',
    'mixed-style@0.1.2': '00000000000000000000000000000000',
    'mixed-style@1.x.x': '00000000000000000000000000000000',
    'mixed-style': '00000000000000000000000000000000',
  }
  const info = (key: keyof typeof patchedDependencies): ExtendedPatchInfo => ({
    key,
    hash: patchedDependencies[key],
  })
  expect(_groupPatchedDependencies(patchedDependencies)).toStrictEqual({
    'exact-version-only': {
      exact: {
        '0.0.0': info('exact-version-only@0.0.0'),
        '1.2.3': info('exact-version-only@1.2.3'),
        '2.1.0': info('exact-version-only@2.1.0'),
      },
      range: [],
      all: undefined,
    },
    'version-range-only': {
      exact: {},
      range: [
        {
          version: '~1.2.0',
          patch: info('version-range-only@~1.2.0'),
        },
        {
          version: '4',
          patch: info('version-range-only@4'),
        },
      ],
      all: undefined,
    },
    'star-version-range': {
      exact: {},
      range: [],
      all: info('star-version-range@*'),
    },
    'without-versions': {
      exact: {},
      range: [],
      all: info('without-versions'),
    },
    'mixed-style': {
      exact: {
        '0.1.2': info('mixed-style@0.1.2'),
      },
      range: [
        {
          version: '1.x.x',
          patch: info('mixed-style@1.x.x'),
        },
      ],
      all: info('mixed-style'),
    },
  } as PatchGroupRecord)
})

test('errors on invalid version range', async () => {
  expect(() => _groupPatchedDependencies({
    'foo@link:packages/foo': '00000000000000000000000000000000',
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_PATCH_NON_SEMVER_RANGE',
  }))
})
