import { createOptionalDependenciesRemover } from '../lib/createOptionalDependenciesRemover'

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
