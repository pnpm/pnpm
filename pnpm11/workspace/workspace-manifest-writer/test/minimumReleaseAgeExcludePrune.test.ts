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

test('remove a versioned entry whose version is not resolved', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0', 'bar@2.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['1.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@1.0.0'],
  })
})

test('keep entries that are resolved', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0', '@foo/bar@2.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['1.0.0'], '@foo/bar': ['2.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@1.0.0', '@foo/bar@2.0.0'],
  })
})

test('rewrite a multi-version entry keeping only the resolved versions', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0 || 2.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['2.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@2.0.0'],
  })
})

test('rewrite a narrowed union in canonical semver order', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@3.0.0 || 1.0.0 || 2.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['3.0.0', '1.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@1.0.0 || 3.0.0'],
  })
})

test('remove a bare-name entry whose package is absent, keep one whose package is present', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo', 'bar@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({ bar: ['1.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['bar@1.0.0'],
  })
})

test('keep glob entries even when nothing matches them', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['@babel/*'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['@babel/*'],
  })
})

test('remove the field when no entry survives', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    onlyBuiltDependencies: ['fsevents'],
    minimumReleaseAgeExclude: ['foo@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    onlyBuiltDependencies: ['fsevents'],
  })
})

test('delete the file when the removed field was the only content', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(fs.existsSync(filePath)).toBeFalsy()
})

test('no cleanup when resolvedPackageVersions is absent', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0', 'bar'],
  })
  await updateWorkspaceManifest(dir, {})
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@1.0.0', 'bar'],
  })
})

test('keep entries that fail to parse', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@not-a-version', 'bar@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    resolvedPackageVersions: resolvedPackageVersions({}),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@not-a-version'],
  })
})

test('entries added in the same write survive the cleanup', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    addedMinimumReleaseAgeExcludes: ['foo@2.0.0'],
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['2.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@2.0.0'],
  })
})

test('entries added in the same write are never pruned, even when absent from the resolved versions', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    minimumReleaseAgeExclude: ['foo@1.0.0'],
  })
  await updateWorkspaceManifest(dir, {
    addedMinimumReleaseAgeExcludes: ['bar@9.9.9'],
    resolvedPackageVersions: resolvedPackageVersions({ foo: ['1.0.0'] }),
    minimumReleaseAgeExcludePrune: true,
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    minimumReleaseAgeExclude: ['foo@1.0.0', 'bar@9.9.9'],
  })
})
