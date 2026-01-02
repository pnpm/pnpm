import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('updateWorkspaceManifest adds a new setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], allowBuilds: {} })
  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: {} },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    allowBuilds: {},
  })
})

test('updateWorkspaceManifest removes an existing setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], overrides: { foo: '2' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { overrides: undefined },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
  })
})

test('updateWorkspaceManifest updates an existing setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], overrides: { foo: '2' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { overrides: { bar: '3' } },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    overrides: { bar: '3' },
  })
})

test('updateWorkspaceManifest updates allowBuilds', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], allowBuilds: { qar: 'warn' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: { foo: true, bar: false } },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    allowBuilds: {
      bar: false,
      foo: true,
    },
  })
})
