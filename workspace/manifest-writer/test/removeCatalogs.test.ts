import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { addCatalogs, removePackagesFromWorkspaceCatalog } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('addCatalogs adds `default` catalogs to the `catalog` object by default and remove `default` catalogs if they are empty', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await addCatalogs(dir, {
    default: {
      foo: '^0.1.2',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      foo: '^0.1.2',
    },
  })

  await removePackagesFromWorkspaceCatalog(dir, {
    foo: 'catalog:',
  })
  expect(readYamlFile(filePath)).toStrictEqual({})
})

test('addCatalogs adds `default` catalogs to the `catalog` object if it exists and remove catalog', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalog: {
      bar: '3.2.1',
    },
  })
  await addCatalogs(dir, {
    default: {
      foo: '^0.1.2',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
      foo: '^0.1.2',
    },
  })

  await removePackagesFromWorkspaceCatalog(dir, {
    foo: 'catalog:',
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
    },
  })
})

test('addCatalogs adds `default` catalogs to the `catalogs.default` object if it exists and remove catalog', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalogs: {
      default: {
        bar: '3.2.1',
      },
    },
  })
  await addCatalogs(dir, {
    default: {
      foo: '^0.1.2',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      default: {
        bar: '3.2.1',
        foo: '^0.1.2',
      },
    },
  })
  await removePackagesFromWorkspaceCatalog(dir, {
    foo: 'catalog:',
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      default: {
        bar: '3.2.1',
      },
    },
  })
})

test('addCatalogs creates a `catalogs` object for any-named catalogs and remove catalog', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await addCatalogs(dir, {
    foo: {
      abc: '0.1.2',
    },
    bar: {
      def: '3.2.1',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      foo: {
        abc: '0.1.2',
      },
      bar: {
        def: '3.2.1',
      },
    },
  })
  await removePackagesFromWorkspaceCatalog(dir, {
    abc: 'catalog:foo',
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      bar: {
        def: '3.2.1',
      },
    },
  })
})

test('addCatalogs add any-named catalogs to the `catalogs` object if it already exists and remove catalog', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalogs: {
      foo: {
        ghi: '7.8.9',
      },
    },
  })
  await addCatalogs(dir, {
    foo: {
      abc: '0.1.2',
    },
    bar: {
      def: '3.2.1',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      foo: {
        abc: '0.1.2',
        ghi: '7.8.9',
      },
      bar: {
        def: '3.2.1',
      },
    },
  })

  await removePackagesFromWorkspaceCatalog(dir, {
    abc: 'catalog:foo',
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      bar: {
        def: '3.2.1',
      },
      foo: {
        ghi: '7.8.9',
      },
    },
  })
  await removePackagesFromWorkspaceCatalog(dir, {
    def: 'catalog:bar',
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalogs: {
      foo: {
        ghi: '7.8.9',
      },
    },
  })
  await removePackagesFromWorkspaceCatalog(dir, {
    ghi: 'catalog:foo',
  })
  expect(readYamlFile(filePath)).toStrictEqual({})
})
