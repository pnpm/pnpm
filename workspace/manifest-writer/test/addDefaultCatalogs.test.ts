import fs from 'fs'
import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { addDefaultCatalogs } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('addDefaultCatalogs does not write new workspace manifest for empty catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await addDefaultCatalogs(dir, {})
  expect(fs.existsSync(filePath)).toBe(false)
})

test('addDefaultCatalogs does not add empty catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {})
  await addDefaultCatalogs(dir, {})
  expect(readYamlFile(filePath)).toStrictEqual({})
})

test('addDefaultCatalogs adds to the `catalog` object by default', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await addDefaultCatalogs(dir, {
    foo: {
      specifier: '^0.1.2',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      foo: '^0.1.2',
    },
  })
})

test('addDefaultCatalogs adds to the `catalog` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalog: {
      bar: '3.2.1',
    },
  })
  await addDefaultCatalogs(dir, {
    foo: {
      specifier: '^0.1.2',
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
      foo: '^0.1.2',
    },
  })
})

test('addDefaultCatalogs adds to the `catalogs.default` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalogs: {
      default: {
        bar: '3.2.1',
      },
    },
  })
  await addDefaultCatalogs(dir, {
    foo: {
      specifier: '^0.1.2',
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
})
