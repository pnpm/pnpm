import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { findPackages } from '@pnpm/workspace.projects-reader'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

test('remove the default catalog if it is empty', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  prepare({
    dependencies: {
      foo: '^0.1.2',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {},
    },
    allProjects,
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
      foo: '^0.1.2',
    },
  })
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(fs.existsSync(filePath)).toBeFalsy()
})

test('remove the unused default catalog', async () => {
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
  prepare({
    dependencies: {
      foo: '^0.1.2',
      bar: 'catalog:',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
    },
  })
})

test('remove the unused default catalog with catalogs', async () => {
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
  prepare({
    dependencies: {
      foo: '^0.1.2',
      bar: 'catalog:',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      default: {
        bar: '3.2.1',
      },
    },
  })
})

test('remove the unused named catalog', async () => {
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
  prepare({
    dependencies: {
      abc: '0.1.2',
      def: 'catalog:bar',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      bar: {
        def: '3.2.1',
      },
    },
  })
})

test('remove all unused named catalogs', async () => {
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
  prepare({
    dependencies: {
      def: 'catalog:bar',
      ghi: 'catalog:foo',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)

  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      bar: {
        def: '3.2.1',
      },
      foo: {
        ghi: '7.8.9',
      },
    },
  })
  prepare({
    dependencies: {
      def: '3.2.1',
    },
  }, { tempDir: dir })
  const _allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects: _allProjects,
  })
  expect(fs.existsSync(filePath)).toBeFalsy()
})

test('same pkg with different version', async () => {
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
        abc: '1.2.3',
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
        abc: '1.2.3',
      },
    },
  })
  prepare({
    dependencies: {
      def: 'catalog:bar',
      ghi: 'catalog:foo',
      abc: 'catalog:foo',
    },
    optionalDependencies: {
      abc: 'catalog:bar',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      bar: {
        abc: '1.2.3',
        def: '3.2.1',
      },
      foo: {
        abc: '0.1.2',
        ghi: '7.8.9',
      },
    },
  })
})

test('update catalogs and remove catalog', async () => {
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
  prepare({
    dependencies: {
      def: 'catalog:bar',
      ghi: 'catalog:foo',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {
        ghi: '7.9.9',
      },
    },
    cleanupUnusedCatalogs: true,
    allProjects,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      foo: {
        ghi: '7.9.9',
      },
      bar: {
        def: '3.2.1',
      },
    },
  })
})

test('when allProjects is undefined should not cleanup unused catalogs', async () => {
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
  prepare({
    dependencies: {
      def: 'catalog:bar',
      ghi: 'catalog:foo',
    },
  }, { tempDir: dir })
  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      foo: {
        ghi: '7.9.9',
      },
    },
    cleanupUnusedCatalogs: true,
    allProjects: undefined,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalogs: {
      foo: {
        abc: '0.1.2',
        ghi: '7.9.9',
      },
      bar: {
        def: '3.2.1',
      },
    },
  })
})

test('keep catalogs referenced only in workspace overrides', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    catalog: {
      foo: '1.0.0',
    },
    catalogs: {
      bar: {
        '@scope/def': '2.0.0',
      },
    },
    overrides: {
      foo: 'catalog:',
      '@scope/parent@1>@scope/def': 'catalog:bar',
    },
  })

  prepare({
    dependencies: {
      zoo: '^1.0.0',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)

  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })

  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalog: {
      foo: '1.0.0',
    },
    catalogs: {
      bar: {
        '@scope/def': '2.0.0',
      },
    },
    overrides: {
      foo: 'catalog:',
      '@scope/parent@1>@scope/def': 'catalog:bar',
    },
  })
})

test('remove catalogs unused by dependencies and workspace overrides', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    catalog: {
      foo: '1.0.0',
      unusedDefault: '2.0.0',
    },
    catalogs: {
      bar: {
        def: '2.0.0',
        unusedNamed: '3.0.0',
      },
    },
    overrides: {
      foo: 'catalog:',
      def: 'catalog:bar',
    },
  })

  prepare({
    dependencies: {
      zoo: '^1.0.0',
    },
  }, { tempDir: dir })
  const allProjects = await findPackages(dir)

  await updateWorkspaceManifest(dir, {
    cleanupUnusedCatalogs: true,
    allProjects,
  })

  expect(readYamlFileSync(filePath)).toStrictEqual({
    catalog: {
      foo: '1.0.0',
    },
    catalogs: {
      bar: {
        def: '2.0.0',
      },
    },
    overrides: {
      foo: 'catalog:',
      def: 'catalog:bar',
    },
  })
})
