import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

test('remove an undecided allowBuilds entry whose package is not resolved', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    allowBuilds: {
      foo: 'set this to true or false',
      bar: 'set this to true or false',
    },
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['1.0.0'] }),
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    allowBuilds: {
      foo: 'set this to true or false',
    },
  })
})

test('keep decided allowBuilds entries even when not resolved', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    allowBuilds: {
      foo: true,
      bar: false,
      baz: 'set this to true or false',
    },
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    allowBuilds: {
      foo: true,
      bar: false,
    },
  })
})

test('delete the allowBuilds block if it becomes empty', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    allowBuilds: {
      foo: 'set this to true or false',
    },
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
  })
  expect(fs.existsSync(filePath)).toBe(false)
})

test('flow-style allowBuilds: undecided entry is pruned, the mapping stays flow-style', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await fs.promises.writeFile(filePath, "allowBuilds: {foo: true, bar: 'set this to true or false'}\n")
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe('allowBuilds: { foo: true }\n')
})

test('flow-style allowBuilds: a trailing comment and original quoting survive the prune', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await fs.promises.writeFile(filePath, "allowBuilds: {foo: 'set this to true or false', bar: true, baz: 'set this to true or false'} # hey\n")
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: [] }),
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe("allowBuilds: { foo: 'set this to true or false', bar: true } # hey\n")
})

test('a dep-path key is pruned by its package name', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000': 'set this to true or false',
      'bar@git+https://github.com/org/bar.git#0000000000000000000000000000000000000000': 'set this to true or false',
    },
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: [] }),
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000': 'set this to true or false',
    },
  })
})

test('keys with no provable package name are kept', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git': 'set this to true or false',
    },
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git': 'set this to true or false',
    },
  })
})

test('an escaped quoted key is pruned by its decoded name', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  await fs.promises.writeFile(filePath, 'allowBuilds:\n  "\\u0066oo": set this to true or false\n  bar: true\n')
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    allowBuilds: { bar: true },
  })
})

function resolvedPackageVersions (entries: Record<string, string[]>): Map<string, Set<string>> {
  return new Map(Object.entries(entries).map(([name, versions]) => [name, new Set(versions)]))
}
