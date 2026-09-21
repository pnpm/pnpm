/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { findWorkspaceDir, findWorkspaceDirSync } from '@pnpm/workspace.root-finder'
import { temporaryDirectory } from 'tempy'

const NPM_CONFIG_WORKSPACE_DIR_ENV_VAR = 'NPM_CONFIG_WORKSPACE_DIR'
const FAKE_PATH = 'FAKE_PATH'
function isFileSystemCaseSensitive () {
  try {
    fs.realpathSync.native(process.cwd().toUpperCase())
    return false
  } catch {
    return true
  }
}

// We don't need to validate case-sensitive systems
// because it is not possible to reach process.cwd() with wrong case there.
const testOnCaseInSensitiveSystems = isFileSystemCaseSensitive() ? test.skip : test

test('finds actual workspace dir', async () => {
  const workspaceDir = await findWorkspaceDir(process.cwd())

  expect(workspaceDir).toBe(path.resolve(import.meta.dirname, '..', '..', '..', '..'))
})

testOnCaseInSensitiveSystems('finds workspace dir with wrong case from cwd', async () => {
  const workspaceDir = await findWorkspaceDir(process.cwd().toUpperCase())

  expect(workspaceDir).toBe(path.resolve(import.meta.dirname, '..', '..', '..', '..'))
})

test('finds overridden workspace dir', async () => {
  const oldValue = process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR]
  process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = FAKE_PATH
  const workspaceDir = await findWorkspaceDir(process.cwd())
  if (oldValue == null) {
    delete process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR]
  } else {
    process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = oldValue
  }

  expect(workspaceDir).toBe(FAKE_PATH)
})

test('does not find a workspace dir for a project the workspace excludes', async () => {
  const workspaceDir = prepareWorkspace(['packages/**', '!examples/**'])

  expect(await findWorkspaceDir(path.join(workspaceDir, 'examples/example-1'))).toBeUndefined()
})

test('does not find a workspace dir for a project the workspace does not list', async () => {
  const workspaceDir = prepareWorkspace(['packages/**'])

  expect(await findWorkspaceDir(path.join(workspaceDir, 'docs'))).toBeUndefined()
})

test('finds the workspace dir for a project the workspace lists', async () => {
  const workspaceDir = prepareWorkspace(['packages/**'])

  expect(await findWorkspaceDir(path.join(workspaceDir, 'packages/pkg-1'))).toBe(workspaceDir)
})

test('finds the workspace dir for a directory without a manifest of its own', async () => {
  const workspaceDir = prepareWorkspace(['packages/**'])

  expect(await findWorkspaceDir(path.join(workspaceDir, 'packages/pkg-1/src'))).toBe(workspaceDir)
})

test('finds the workspace dir from the workspace root that no pattern lists', async () => {
  const workspaceDir = prepareWorkspace(['packages/**'])

  expect(await findWorkspaceDir(workspaceDir)).toBe(workspaceDir)
})

test('findWorkspaceDirSync finds actual workspace dir', () => {
  const workspaceDir = findWorkspaceDirSync(process.cwd())

  expect(workspaceDir).toBe(path.resolve(import.meta.dirname, '..', '..', '..', '..'))
})

test('findWorkspaceDirSync behaves identically to findWorkspaceDir on prepared workspace', () => {
  const workspaceDir = prepareWorkspace(['packages/**', '!examples/**'])

  expect(findWorkspaceDirSync(path.join(workspaceDir, 'packages/pkg-1'))).toBe(workspaceDir)
  expect(findWorkspaceDirSync(path.join(workspaceDir, 'packages/pkg-1/src'))).toBe(workspaceDir)
  expect(findWorkspaceDirSync(path.join(workspaceDir, 'examples/example-1'))).toBeUndefined()
  expect(findWorkspaceDirSync(path.join(workspaceDir, 'docs'))).toBeUndefined()
})

function prepareWorkspace (packages: string[]): string {
  const workspaceDir = fs.realpathSync.native(temporaryDirectory())
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-workspace.yaml'), `packages:\n${packages.map((pattern) => `  - '${pattern}'\n`).join('')}`)
  for (const projectDir of ['.', 'packages/pkg-1', 'examples/example-1', 'docs']) {
    fs.mkdirSync(path.join(workspaceDir, projectDir), { recursive: true })
    fs.writeFileSync(path.join(workspaceDir, projectDir, 'package.json'), '{"name":"test","version":"0.0.0"}')
  }
  fs.mkdirSync(path.join(workspaceDir, 'packages/pkg-1/src'))
  return workspaceDir
}
