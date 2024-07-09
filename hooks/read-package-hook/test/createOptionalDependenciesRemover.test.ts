import { createOptionalDependenciesRemover } from '../lib/createOptionalDependenciesRemover'
import type { BaseManifest } from '@pnpm/types'

test('createOptionalDependenciesRemover() does not modify the manifest if provided array is empty', async () => {
  const removeOptionalDependencies = createOptionalDependenciesRemover([])
  const manifest: BaseManifest = Object.freeze({
    dependencies: {
      foo: '0.1.2',
      bar: '2.1.0',
      baz: '1.0.0',
      qux: '2.0.0',
    },
    optionalDependencies: {
      foo: '0.1.2',
      bar: '2.1.0',
      baz: '1.0.0',
    },
  })
  expect(await removeOptionalDependencies(manifest)).toBe(manifest)
})

test('createOptionalDependenciesRemover() removes optional dependencies', async () => {
  const removeOptionalDependencies = createOptionalDependenciesRemover(['foo', 'bar'])
  expect(
    await removeOptionalDependencies({
      dependencies: {
        foo: '0.1.2',
        bar: '2.1.0',
        baz: '1.0.0',
        qux: '2.0.0',
      },
      optionalDependencies: {
        foo: '0.1.2',
        bar: '2.1.0',
        baz: '1.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      baz: '1.0.0',
      qux: '2.0.0',
    },
    optionalDependencies: {
      baz: '1.0.0',
    },
  })
})

test('createOptionalDependenciesRemover() does not remove non-optional packages', async () => {
  const removeOptionalDependencies = createOptionalDependenciesRemover(['foo', 'bar'])
  expect(
    await removeOptionalDependencies({
      dependencies: {
        foo: '0.1.2',
        bar: '2.1.0',
        baz: '1.0.0',
        qux: '2.0.0',
      },
      optionalDependencies: {
        foo: '0.1.2',
        baz: '1.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      bar: '2.1.0',
      baz: '1.0.0',
      qux: '2.0.0',
    },
    optionalDependencies: {
      baz: '1.0.0',
    },
  })
})

test('createOptionalDependenciesRemover() removes all optional dependencies if the pattern is a star', async () => {
  const removeOptionalDependencies = createOptionalDependenciesRemover(['*'])
  expect(
    await removeOptionalDependencies({
      dependencies: {
        foo: '0.1.2',
        bar: '2.1.0',
        baz: '1.0.0',
        qux: '2.0.0',
      },
      optionalDependencies: {
        foo: '0.1.2',
        bar: '2.1.0',
        baz: '1.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      qux: '2.0.0',
    },
    optionalDependencies: {},
  })
})

test('createOptionalDependenciesRemover() only removes optional dependencies that match one of the patterns', async () => {
  const removeOptionalDependencies = createOptionalDependenciesRemover(['@foo/*', '@bar/*'])
  expect(
    await removeOptionalDependencies({
      dependencies: {
        '@foo/abc': '0.0.0',
        '@foo/def': '0.0.0',
        '@foo/not-optional': '0.0.0',
        '@bar/ghi': '0.0.0',
        '@bar/required': '0.0.0',
        '@baz/jkl': '0.0.0',
      },
      optionalDependencies: {
        '@foo/abc': '0.0.0',
        '@foo/def': '0.0.0',
        '@bar/ghi': '0.0.0',
        '@baz/jkl': '0.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      '@foo/not-optional': '0.0.0',
      '@bar/required': '0.0.0',
      '@baz/jkl': '0.0.0',
    },
    optionalDependencies: {
      '@baz/jkl': '0.0.0',
    },
  })
})
