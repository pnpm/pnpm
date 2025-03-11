import { getPatchInfo } from '../src/getPatchInfo'
import { type PatchGroupRecord } from '../src/index'

test('getPatchInfo(undefined, ...) returns undefined', () => {
  expect(getPatchInfo(undefined, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns exact version if match', () => {
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
      range: {},
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.exact['1.0.0'])
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns range version if satisfied', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: {
        1: {
          file: {
            path: 'patches/foo@1.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1',
          strict: true,
        },
      },
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(getPatchInfo(patchedDependencies, 'foo', '1.0.0')).toStrictEqual(patchedDependencies.foo.range['1'])
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.0')).toStrictEqual(patchedDependencies.foo.range['1'])
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toBeUndefined()
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo() returns "all" if name matches', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: {},
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

test('exact version overrides version range, version range overrides "all"', () => {
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
      range: {
        1: {
          file: {
            path: 'patches/foo@1.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@1',
          strict: true,
        },
        2: {
          file: {
            path: 'patches/foo@2.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@2',
          strict: true,
        },
      },
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
  expect(getPatchInfo(patchedDependencies, 'foo', '1.1.1')).toStrictEqual(patchedDependencies.foo.range[1])
  expect(getPatchInfo(patchedDependencies, 'foo', '2.0.0')).toStrictEqual(patchedDependencies.foo.range[2])
  expect(getPatchInfo(patchedDependencies, 'foo', '2.1.0')).toStrictEqual(patchedDependencies.foo.range[2])
  expect(getPatchInfo(patchedDependencies, 'foo', '3.0.0')).toStrictEqual(patchedDependencies.foo.all)
  expect(getPatchInfo(patchedDependencies, 'bar', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) throws an error when name@version matches more than one version ranges', () => {
  const patchedDependencies = {
    foo: {
      exact: {},
      range: {
        '>=1.0.0 <3.0.0': {
          file: {
            path: 'patches/foo_a.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@>=1.0.0 <3.0.0',
          strict: true,
        },
        '>=2.0.0': {
          file: {
            path: 'patches/foo_a.patch',
            hash: '00000000000000000000000000000000',
          },
          key: 'foo@>=2.0.0',
          strict: true,
        },
      },
      all: undefined,
    },
  } satisfies PatchGroupRecord
  expect(() => getPatchInfo(patchedDependencies, 'foo', '2.1.0')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_PATCH_KEY_CONFLICT',
    message: 'Unable to choose between 2 version ranges to patch foo@2.1.0: >=1.0.0 <3.0.0, >=2.0.0',
    hint: 'Explicitly set the exact version (foo@2.1.0) to resolve conflict',
  }))
})
