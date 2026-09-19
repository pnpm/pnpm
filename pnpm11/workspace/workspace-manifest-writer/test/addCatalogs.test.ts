import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
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

test.each([
  [{ other: '1.0.0' }, 'catalog:\n  other: 1.0.0\n  react: &react ^1.0.0\n  react-dom: *react\n'],
  [{ react: '^2.0.0', 'react-dom': '^2.0.0' }, 'catalog:\n  react: &react ^2.0.0\n  react-dom: *react\n'],
  [{ 'react-dom': '^2.0.0' }, 'catalog:\n  react: &react ^1.0.0\n  react-dom: ^2.0.0\n'],
])('preserves compatible catalog scalar aliases for %j', async (updated, expected) => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  fs.writeFileSync(filePath, 'catalog:\n  react: &react ^1.0.0\n  react-dom: *react\n')
  await updateWorkspaceManifest(dir, { updatedCatalogs: { default: updated } })
  expect(fs.readFileSync(filePath, 'utf8')).toBe(expected)
})

test('pruning a catalog anchor promotes its first surviving alias', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  fs.writeFileSync(filePath, "catalog:\n  react: &react '^1.0.0' # definition\n  react-dom: *react # consumer\n  react-is: *react\n")
  await updateWorkspaceManifest(dir, {
    catalogPrune: true,
    allProjects: [{
      rootDir: dir,
      manifest: { dependencies: { 'react-dom': 'catalog:', 'react-is': 'catalog:' } },
    }],
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe("catalog:\n  react-dom: &react '^1.0.0' # consumer\n  react-is: *react\n")
})

test.each([
  'catalog: {react: &version ^1.0.0, react-dom: *version}\n',
  'catalog:\n  react: &version !!str 1.0\n  react-dom: *version\n',
])('updating a scalar anchor leaves other catalog entries unchanged: %s', async (original) => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  fs.writeFileSync(filePath, original)
  const before = readYamlFileSync<{ catalog: Record<string, string> }>(filePath)
  await updateWorkspaceManifest(dir, { updatedCatalogs: { default: { react: '^2.0.0' } } })
  expect(readYamlFileSync(filePath)).toEqual({ catalog: { ...before.catalog, react: '^2.0.0' } })
})

test('preserves aliases across named catalogs', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  const original = 'catalogs:\n  first:\n    react: &react ^1.0.0\n  second:\n    react-dom: *react\n'
  fs.writeFileSync(filePath, original)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: { first: { react: '^2.0.0' }, second: { 'react-dom': '^2.0.0' } },
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe(original.replaceAll('^1.0.0', '^2.0.0'))
})

test('pruning a catalog keeps aliases in other settings valid', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  fs.writeFileSync(filePath, 'catalog:\n  unused: &version ^1.0.0\noverrides:\n  react: *version\n')
  await updateWorkspaceManifest(dir, {
    catalogPrune: true,
    allProjects: [{ rootDir: dir, manifest: { dependencies: { other: '1.0.0' } } }],
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe('overrides:\n  react: &version ^1.0.0\n')
})
