import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

test('updateWorkspaceManifest adds a new setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, { packages: ['*'], allowBuilds: {} })
  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: {} },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    packages: ['*'],
    allowBuilds: {},
  })
})

test('updateWorkspaceManifest removes an existing setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, { packages: ['*'], overrides: { foo: '2' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { overrides: undefined },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    packages: ['*'],
  })
})

test('updateWorkspaceManifest updates an existing setting', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, { packages: ['*'], overrides: { foo: '2' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { overrides: { bar: '3' } },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  writeYamlFileSync(filePath, { packages: ['*'], allowBuilds: { qar: 'warn' } })
  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: { foo: true, bar: false } },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  writeYamlFileSync(filePath, { packages: ['*'] })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { foo: '1.0.0', bar: '2.0.0' },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  writeYamlFileSync(filePath, { packages: ['*'], overrides: { existing: '1.0.0' } })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { newPkg: '2.0.0' },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
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
  writeYamlFileSync(filePath, { packages: ['*'], overrides: { foo: '1.0.0', bar: '1.0.0' } })
  await updateWorkspaceManifest(dir, {
    updatedOverrides: { foo: '2.0.0' },
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
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

  expect(readYamlFileSync(filePath)).toStrictEqual({
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

test('updateWorkspaceManifest preserves blank lines between top-level fields when inserting a new field', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'

allowBuilds:
  foo: true

overrides:
  foo: '1.0.0'
`

  const expected = `\
packages:
  - '*'

allowBuilds:
  foo: true

catalog:
  bar: 2.0.0

overrides:
  foo: '1.0.0'
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: { catalog: { bar: '2.0.0' } },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest preserves blank lines when appending a new field at the end', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  // overrides before catalog is not alphabetical, so the layout is "unordered"
  // and new keys are appended at the end rather than sorted in.
  const manifest = `\
overrides:
  foo: '1.0.0'

catalog:
  bar: '2.0.0'
`

  const expected = `\
overrides:
  foo: '1.0.0'

catalog:
  bar: '2.0.0'

allowBuilds:
  baz: true
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: { baz: true } },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest preserves blank lines when a new key sorts to the front', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  // Sorted-alphabetical layout (no `packages` first). Inserting `allowBuilds`
  // sorts it to the front, demoting `catalog` to the second position. The
  // blank-line style of the original document should still be applied.
  const manifest = `\
catalog:
  bar: '1.0.0'

overrides:
  foo: '2.0.0'
`

  const expected = `\
allowBuilds:
  baz: true

catalog:
  bar: '1.0.0'

overrides:
  foo: '2.0.0'
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: { allowBuilds: { baz: true } },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest does not add blank lines when the original layout has none', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'
allowBuilds:
  foo: true
`

  const expected = `\
packages:
  - '*'
allowBuilds:
  foo: true
catalog:
  bar: 2.0.0
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: { catalog: { bar: '2.0.0' } },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest preserves a fully alphabetical top-level layout (packages not first)', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
allowBuilds:
  foo: true
catalog:
  bar: '1.0.0'
packages:
  - '*'
`

  const expected = `\
allowBuilds:
  foo: true
catalog:
  bar: '1.0.0'
overrides:
  baz: 1.0.0
packages:
  - '*'
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedOverrides: { baz: '1.0.0' },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest inserts new keys in sorted position when existing keys are sorted', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'

overrides:
  apple: '1.0.0'
  mango: '2.0.0'
  zebra: '3.0.0'
`

  const expected = `\
packages:
  - '*'

overrides:
  apple: '1.0.0'
  banana: 4.0.0
  mango: '2.0.0'
  zebra: '3.0.0'
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedOverrides: { banana: '4.0.0' },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})

test('updateWorkspaceManifest preserves the order of existing keys', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)

  const manifest = `\
packages:
  - '*'

overrides:
  zebra: '1.0.0'
  apple: '2.0.0'
  mango: '3.0.0'
`

  const expected = `\
packages:
  - '*'

overrides:
  zebra: '1.5.0'
  apple: '2.5.0'
  mango: '3.5.0'
  banana: 4.0.0
`

  fs.writeFileSync(filePath, manifest)

  await updateWorkspaceManifest(dir, {
    updatedFields: {
      overrides: {
        apple: '2.5.0',
        banana: '4.0.0',
        mango: '3.5.0',
        zebra: '1.5.0',
      },
    },
  })

  expect(fs.readFileSync(filePath).toString()).toStrictEqual(expected)
})
