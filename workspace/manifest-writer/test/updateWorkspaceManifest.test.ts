import fs from 'fs'
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

// This test is intentionally minimal and doesn't exhaustively cover every case
// of comment preservation in pnpm-workspace.yaml.
//
// The tests in @pnpm/yaml.document-sync should cover more cases and be
// sufficient. It's likely not necessary to duplicate the tests in that package.
test('updateWorkspaceManifest preserves comments', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'

overrides:
  bar: '2'
  # This comment on foo should be preserved
  foo: '3'
`

  const expected = `\
packages:
  - '*'

overrides:
  bar: '3'
  baz: '1'
  # This comment on foo should be preserved
  foo: '2'
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: { overrides: { foo: '2', bar: '3', baz: '1' } },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
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

test('updateWorkspaceManifest with updatedOverrides adds overrides when none exist', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'] })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { foo: '1.0.0', bar: '2.0.0' },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    overrides: {
      bar: '2.0.0',
      foo: '1.0.0',
    },
  })
})

test('updateWorkspaceManifest with updatedOverrides merges into existing overrides', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], overrides: { existing: '1.0.0' } })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { newPkg: '2.0.0' },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    overrides: {
      existing: '1.0.0',
      newPkg: '2.0.0',
    },
  })
})

test('updateWorkspaceManifest with updatedOverrides updates existing override values', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'], overrides: { foo: '1.0.0', bar: '1.0.0' } })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { foo: '2.0.0' },
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    overrides: {
      bar: '1.0.0',
      foo: '2.0.0',
    },
  })
})

test('updateWorkspaceManifest with updatedOverrides does not update when values are equal', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  const originalContent = 'packages:\n  - \'*\'\noverrides:\n  foo: \'1.0.0\'\n'
  fs.writeFileSync(filePath, originalContent)
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { foo: '1.0.0' },
  })
  expect(fs.readFileSync(filePath).toString()).toStrictEqual(originalContent)
})

test('updateWorkspaceManifest with updatedOverrides preserves comments', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'

overrides:
  # Comment on existing
  existing: '1.0.0'
`

  const expected = `\
packages:
  - '*'

overrides:
  # Comment on existing
  existing: '1.0.0'
  newPkg: ^2.0.0
`

  fs.writeFileSync(filePath, manifest)
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { newPkg: '^2.0.0' },
  })
  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
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
