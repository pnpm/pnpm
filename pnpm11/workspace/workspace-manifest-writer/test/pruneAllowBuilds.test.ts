import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

function resolvedPackageVersions (entries: Record<string, string[]>): Map<string, Set<string>> {
  return new Map(Object.entries(entries).map(([name, versions]) => [name, new Set(versions)]))
}

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
