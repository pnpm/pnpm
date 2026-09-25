import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

test('write the list to the deprecated auditConfig.ignoreGhsas when that is where it lives', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    auditConfig: { ignoreGhsas: ['GHSA-aaaa-bbbb-cccc'] },
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: ['GHSA-dddd-eeee-ffff'],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    auditConfig: { ignoreGhsas: ['GHSA-dddd-eeee-ffff'] },
  })
})

test('create auditConfig.ignoreGhsas when neither spelling is present', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    packages: ['.'],
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: ['GHSA-dddd-eeee-ffff'],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    packages: ['.'],
    auditConfig: { ignoreGhsas: ['GHSA-dddd-eeee-ffff'] },
  })
})

test('write the list to the canonical audit.ignore when that is where it lives', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    audit: { ignorePrune: true, ignore: ['GHSA-aaaa-bbbb-cccc'] },
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: ['GHSA-dddd-eeee-ffff'],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    audit: { ignorePrune: true, ignore: ['GHSA-dddd-eeee-ffff'] },
  })
})

test('remove the shadowed deprecated list when both spellings are present', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    audit: { ignore: ['GHSA-aaaa-bbbb-cccc'] },
    auditConfig: { ignoreGhsas: ['GHSA-1111-2222-3333'] },
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: ['GHSA-dddd-eeee-ffff'],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    audit: { ignore: ['GHSA-dddd-eeee-ffff'] },
  })
})

test('an empty list removes audit.ignore and keeps its siblings', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    audit: { ignorePrune: true, ignore: ['GHSA-aaaa-bbbb-cccc'] },
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: [],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    audit: { ignorePrune: true },
  })
})

test('an empty list removes the audit block when ignore is its only key', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFileSync(filePath, {
    packages: ['.'],
    audit: { ignore: ['GHSA-aaaa-bbbb-cccc'] },
  })
  await updateWorkspaceManifest(dir, {
    updatedAuditIgnoreGhsas: [],
  })
  expect(readYamlFileSync(filePath)).toStrictEqual({
    packages: ['.'],
  })
})
