import path from 'path'
import fs from 'fs'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { prepare } from '@pnpm/prepare'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { findPackages } from '@pnpm/fs.find-packages'

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
  expect(readYamlFile(filePath)).toStrictEqual({
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
  expect(readYamlFile(filePath)).toStrictEqual({
    catalog: {
      bar: '3.2.1',
    },
  })
})

test('remove the unused default catalog with catalogs', async () => {
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
  expect(readYamlFile(filePath)).toStrictEqual({
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
  expect(readYamlFile(filePath)).toStrictEqual({
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
        abc: '1.2.3',
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
  expect(readYamlFile(filePath)).toStrictEqual({
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
  expect(readYamlFile(filePath)).toStrictEqual({
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
  expect(readYamlFile(filePath)).toStrictEqual({
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
