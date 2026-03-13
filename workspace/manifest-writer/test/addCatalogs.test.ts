import fs from 'node:fs'
import path from 'node:path'

import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

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
  writeYamlFileSync(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {},
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({})
})

test('addCatalogs does not add empty default catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {},
    },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({})
})

test('addCatalogs does not add empty any-named catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {})
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {},
      bar: {},
    },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({})
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
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalog: {
      foo: '^0.1.2',
    },
  })
})

test('addCatalogs adds `default` catalogs to the `catalog` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
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
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
      foo: '^0.1.2',
    },
  })
})

test('addCatalogs adds `default` catalogs to the `catalogs.default` object if it exists', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
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
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  writeYamlFileSync(filePath, {
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
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
