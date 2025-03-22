import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('updateWorkspaceManifest adds a new setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'] })
  await updateWorkspaceManifest(dir, {
    onlyBuiltDependencies: [],
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    onlyBuiltDependencies: [],
  })
})

test('updateWorkspaceManifest removes an existing setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], overrides: { foo: '2' } })
  await updateWorkspaceManifest(dir, {
    overrides: undefined,
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
    overrides: { bar: '3' },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    overrides: { bar: '3' },
  })
})
