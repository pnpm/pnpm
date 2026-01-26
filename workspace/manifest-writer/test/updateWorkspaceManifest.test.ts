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
    updatedFields: { onlyBuiltDependencies: [] },
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
    updatedFields: { onlyBuiltDependencies: ['foo'], ignoredBuiltDependencies: ['bar'] },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    allowBuilds: {
      bar: false,
      foo: true,
      qar: 'warn',
    },
  })
})

test('updateWorkspaceManifest adds a new catalog', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  fs.writeFileSync(filePath, 'packages:\n  - \'*\'\n')

  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {
        foo: '1.0.0',
      },
    },
  })

  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    catalog: { foo: '1.0.0' },
  })
})

test('updateWorkspaceManifest preserves quotes', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
catalog:
  "bar": "2.0.0"
  'foo': '1.0.0'
  qar: 3.0.0
`

  const expected = `\
catalog:
  "bar": "2.0.0"
  'foo': '1.0.0'
  qar: 3.0.0
  zoo: 4.0.0
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedCatalogs: {
      default: {
        foo: '1.0.0',
        bar: '2.0.0',
        qar: '3.0.0',
        zoo: '4.0.0',
      },
    },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})
