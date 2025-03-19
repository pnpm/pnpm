import { type ExtendedPatchInfo, type PatchFile, type PatchGroupRecord } from '@pnpm/patching.types'
import { groupPatchedDependencies } from '../src/groupPatchedDependencies'

function sanitizePatchGroupRecord (patchGroups: PatchGroupRecord): PatchGroupRecord {
  for (const name in patchGroups) {
    patchGroups[name].range.sort((a, b) => a.version.localeCompare(b.version))
  }
  return patchGroups
}

const _groupPatchedDependencies: typeof groupPatchedDependencies = patchedDependencies => sanitizePatchGroupRecord(groupPatchedDependencies(patchedDependencies))

test('groups patchedDependencies according to names, match types, and versions', () => {
  const patchedDependencies = {
    'exact-version-only@0.0.0': {
      hash: '00000000000000000000000000000000',
      path: 'patches/exact-version-only@2.10.patch',
    },
    'exact-version-only@1.2.3': {
      hash: '00000000000000000000000000000000',
      path: 'patches/exact-version-only@1.2.3.patch',
    },
    'exact-version-only@2.1.0': {
      hash: '00000000000000000000000000000000',
      path: 'patches/exact-version-only@2.10.patch',
    },
    'version-range-only@~1.2.0': {
      hash: '00000000000000000000000000000000',
      path: 'patches/version-range-only@~1.2.0.patch',
    },
    'version-range-only@4': {
      hash: '00000000000000000000000000000000',
      path: 'patches/version-range-only@4.patch',
    },
    'star-version-range@*': {
      hash: '00000000000000000000000000000000',
      path: 'patches/star-version-range.patch',
    },
    'without-versions': {
      hash: '00000000000000000000000000000000',
      path: 'patches/without-versions.patch',
    },
    'mixed-style@0.1.2': {
      hash: '00000000000000000000000000000000',
      path: 'patches/mixed-style@0.1.2.patch',
    },
    'mixed-style@1.x.x': {
      hash: '00000000000000000000000000000000',
      path: 'patches/mixed-style@1.x.x.patch',
    },
    'mixed-style': {
      hash: '00000000000000000000000000000000',
      path: 'patches/mixed-style.patch',
    },
  } satisfies Record<string, PatchFile>
  const info = (strict: boolean, key: keyof typeof patchedDependencies): ExtendedPatchInfo => ({
    strict,
    key,
    file: patchedDependencies[key],
  })
  expect(_groupPatchedDependencies(patchedDependencies)).toStrictEqual({
    'exact-version-only': {
      exact: {
        '0.0.0': info(true, 'exact-version-only@0.0.0'),
        '1.2.3': info(true, 'exact-version-only@1.2.3'),
        '2.1.0': info(true, 'exact-version-only@2.1.0'),
      },
      range: [],
      all: undefined,
    },
    'version-range-only': {
      exact: {},
      range: [
        {
          version: '~1.2.0',
          patch: info(true, 'version-range-only@~1.2.0'),
        },
        {
          version: '4',
          patch: info(true, 'version-range-only@4'),
        },
      ],
      all: undefined,
    },
    'star-version-range': {
      exact: {},
      range: [],
      all: info(true, 'star-version-range@*'),
    },
    'without-versions': {
      exact: {},
      range: [],
      all: info(false, 'without-versions'),
    },
    'mixed-style': {
      exact: {
        '0.1.2': info(true, 'mixed-style@0.1.2'),
      },
      range: [
        {
          version: '1.x.x',
          patch: info(true, 'mixed-style@1.x.x'),
        },
      ],
      all: info(false, 'mixed-style'),
    },
  } as PatchGroupRecord)
})

test('errors on invalid version range', async () => {
  expect(() => _groupPatchedDependencies({
    'foo@link:packages/foo': {
      hash: '00000000000000000000000000000000',
      path: 'patches/foo.patch',
    },
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_PATCH_NON_SEMVER_RANGE',
  }))
})
