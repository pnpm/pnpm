import fs from 'fs'
import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('addCatalogs does not write new workspace manifest for empty catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await updateWorkspaceManifest(dir, {})
  expect(fs.existsSync(filePath)).toBe(false)
})

test('addCatalogs does not write new workspace manifest for empty default catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {},
    },
  })
  expect(fs.existsSync(filePath)).toBe(false)
})

test('addCatalogs does not write new workspace manifest for empty any-named catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {},
      bar: {},
    },
  })
  expect(fs.existsSync(filePath)).toBe(false)
})

test('addCatalogs does not add empty catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {},
  })
  expect(readYamlFile(filePath)).toStrictEqual({})
})

test('addCatalogs does not add empty default catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {},
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({})
})

test('addCatalogs does not add empty any-named catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {},
      bar: {},
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({})
})

test('addCatalogs adds `default` catalogs to the `catalog` object by default', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {
        foo: '^0.1.2',
      },
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      foo: '^0.1.2',
    },
  })
})

test('addCatalogs adds `default` catalogs to the `catalog` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalog: {
      bar: '3.2.1',
    },
  })
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {
        foo: '^0.1.2',
      },
    },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
      foo: '^0.1.2',
    },
  })
})

test('addCatalogs adds `default` catalogs to the `catalogs.default` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalogs: {
      default: {
        bar: '3.2.1',
      },
    },
  })
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {
        foo: '^0.1.2',
      },
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

test('addCatalogs creates a `catalogs` object for any-named catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {
        abc: '0.1.2',
      },
      bar: {
        def: '3.2.1',
      },
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
})

test('addCatalogs add any-named catalogs to the `catalogs` object if it already exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, {
    catalogs: {
      foo: {
        ghi: '7.8.9',
      },
    },
  })
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {
        abc: '0.1.2',
      },
      bar: {
        def: '3.2.1',
      },
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
})
