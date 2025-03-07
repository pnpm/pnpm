import { type PatchFile } from '@pnpm/patching.types'
import { type PatchFileGroupRecord, groupPatchedDependencies } from '../src/groupPatchedDependencies'

test('groups patchedDependencies according to names, version selectors, and versions', () => {
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
  expect(groupPatchedDependencies(patchedDependencies)).toStrictEqual({
    'exact-version-only': {
      exact: {
        '0.0.0': patchedDependencies['exact-version-only@0.0.0'],
        '1.2.3': patchedDependencies['exact-version-only@1.2.3'],
        '2.1.0': patchedDependencies['exact-version-only@2.1.0'],
      },
      range: {},
      blank: undefined,
    },
    'version-range-only': {
      exact: {},
      range: {
        '~1.2.0': patchedDependencies['version-range-only@~1.2.0'],
        '4': patchedDependencies['version-range-only@4'],
      },
      blank: undefined,
    },
    'star-version-range': {
      exact: {},
      range: {},
      blank: patchedDependencies['star-version-range@*'],
    },
    'without-versions': {
      exact: {},
      range: {},
      blank: patchedDependencies['without-versions'],
    },
    'mixed-style': {
      exact: {
        '0.1.2': patchedDependencies['mixed-style@0.1.2'],
      },
      range: {
        '1.x.x': patchedDependencies['mixed-style@1.x.x'],
      },
      blank: patchedDependencies['mixed-style'],
    },
  } as PatchFileGroupRecord)
})
