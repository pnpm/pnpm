import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'

test('combines implicit default and named catalogs', () => {
  expect(getCatalogsFromWorkspaceManifest({
    catalog: {
      foo: '^1.0.0',
    },
    catalogs: {
      bar: {
        baz: '^2.0.0',
      },
    },
  })).toEqual({
    default: {
      foo: '^1.0.0',
    },
    bar: {
      baz: '^2.0.0',
    },
  })
})

test('combines explicit default and named catalogs', () => {
  expect(getCatalogsFromWorkspaceManifest({
    catalogs: {
      default: {
        foo: '^1.0.0',
      },
      bar: {
        baz: '^2.0.0',
      },
    },
  })).toEqual({
    default: {
      foo: '^1.0.0',
    },
    bar: {
      baz: '^2.0.0',
    },
  })
})

test('throws if default catalog is defined multiple times', () => {
  expect(() => getCatalogsFromWorkspaceManifest({
    catalog: {
      bar: '^2.0.0',
    },
    catalogs: {
      default: {
        foo: '^1.0.0',
      },
    },
  })).toThrow(/The 'default' catalog was defined multiple times/)
})
