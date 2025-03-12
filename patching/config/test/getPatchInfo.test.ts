import { getPatchInfo } from '../src/getPatchInfo'
import { type PatchGroupRecord } from '../src/index'

test('getPatchInfo(undefined, ...) returns undefined', () => {
  expect(getPatchInfo(undefined, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns an exact version patch if the name and version match', () => {
  const patchedDependencies = {
    foo: {
      exact: {
        '1.0.0': {
          file: {
            path: 'patches/foo@1.0.0.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1.0.0',
          strict: true,
        },
      },
      range: [],
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.exact['1.0.0'])
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns a range version patch if the name matches and the version satisfied', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: [{
        version: '1',
        patch: {
          file: {
            path: 'patches/foo@1.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1',
          strict: true,
        },
      }],
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.range[0].patch)
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toStrictEqual(patchedDependencies.foo.range[0].patch)
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns name-only patch if the name matches', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: [],
      all: {
        file: {
          path: 'patches/foo.patch',
          hash: '00000000000000000000000000000000',
        },
        key: 'foo',
        strict: true,
      },
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.all)
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toStrictEqual(patchedDependencies.foo.all)
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toStrictEqual(patchedDependencies.foo.all)
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('exact version patches override version range patches, version range patches override name-only patches', () => {
  const patchedDependencies = {
    foo: {
      exact: {
        '1.0.0': {
          file: {
            path: 'patches/foo@1.0.0.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1.0.0',
          strict: true,
        },
        '1.1.0': {
          file: {
            path: 'patches/foo@1.1.0.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1.1.0',
          strict: true,
        },
      },
      range: [
        {
          version: '1',
          patch: {
            file: {
              path: 'patches/foo@1.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@1',
            strict: true,
          },
        },
        {
          version: '2',
          patch: {
            file: {
              path: 'patches/foo@2.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@2',
            strict: true,
          },
        },
      ],
      all: {
        file: {
          path: 'patches/foo.patch',
          hash: '00000000000000000000000000000000',
        },
        key: 'foo',
        strict: true,
      },
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.exact['1.0.0'])
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toStrictEqual(patchedDependencies.foo.exact['1.1.0'])
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.1')).toStrictEqual(patchedDependencies.foo.range[0].patch)
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toStrictEqual(patchedDependencies.foo.range[1].patch)
  expect(getPatchInfo(patchedDependencies, 'foo', '2.1.0')).toStrictEqual(patchedDependencies.foo.range[1].patch)
  expect(getPatchInfo(patchedDependencies, 'foo', '3.0.0')).toStrictEqual(patchedDependencies.foo.all)
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) throws an error when name@version matches more than one version range patches', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: [
        {
          version: '>=1.0.0 <3.0.0',
          patch: {
            file: {
              path: 'patches/foo_a.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@>=1.0.0 <3.0.0',
            strict: true,
          },
        },
        {
          version: '>=2.0.0',
          patch: {
            file: {
              path: 'patches/foo_b.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@>=2.0.0',
            strict: true,
          },
        },
      ],
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(() => getPatchInfo(patchedDependencies, 'foo', '2.1.0')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_PATCH_KEY_CONFLICT',
    message: 'Unable to choose between 2 version ranges to patch foo@2.1.0: >=1.0.0 <3.0.0, >=2.0.0',
    hint: 'Explicitly set the exact version (foo@2.1.0) to resolve conflict',
  }))
})

test('getPatchInfo(_, name, version) does not throw an error when name@version matches an exact version patch and more than one version range patches', () => {
  const patchedDependencies = {
    foo: {
      exact: {
        '2.1.0': {
          file: {
            path: 'patches/foo_a.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@>=1.0.0 <3.0.0',
          strict: true,
        },
      },
      range: [
        {
          version: '>=1.0.0 <3.0.0',
          patch: {
            file: {
              path: 'patches/foo_b.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@>=1.0.0 <3.0.0',
            strict: true,
          },
        },
        {
          version: '>=2.0.0',
          patch: {
            file: {
              path: 'patches/foo_c.patch',
              hash: '00000000000000000000000000000000',
            },
            key: 'foo@>=2.0.0',
            strict: true,
          },
        },
      ],
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '2.1.0')).toStrictEqual(patchedDependencies.foo.exact['2.1.0'])
})
