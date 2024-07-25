import { deployCatalogHook } from '../src/deployCatalogHook'

test('deployCatalogHook()', () => {
  const catalogs = {
    default: {
      a: '^1.0.0',
    },
    foo: {
      b: '^2.0.0',
    },
    bar: {
      c: '^3.0.0',
    },
  }

  expect(deployCatalogHook(catalogs, {
    dependencies: {
      a: 'catalog:',
    },
    devDependencies: {
      b: 'catalog:foo',
    },
    optionalDependencies: {
      c: 'catalog:bar',
    },
  })).toStrictEqual({
    dependencies: {
      a: '^1.0.0',
    },
    devDependencies: {
      b: '^2.0.0',
    },
    optionalDependencies: {
      c: '^3.0.0',
    },
  })
})
