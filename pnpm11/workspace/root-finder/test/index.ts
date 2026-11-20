/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { findWorkspaceDir, findWorkspaceDirSync } from '@pnpm/workspace.root-finder'
import { temporaryDirectory } from 'tempy'

const FAKE_PATH = 'FAKE_PATH'
const WORKSPACE_DIR_ENV_VARS = [
  'PNPM_CONFIG_WORKSPACE_DIR',
  'pnpm_config_workspace_dir',
  'NPM_CONFIG_WORKSPACE_DIR',
  'npm_config_workspace_dir',
]
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

test.each(WORKSPACE_DIR_ENV_VARS)('finds workspace dir overridden by %s', async (envVar) => {
  const workspaceDir = await withWorkspaceDirEnv({ [envVar]: FAKE_PATH }, () => findWorkspaceDir(process.cwd()))

  expect(workspaceDir).toBe(FAKE_PATH)
  expect(await withWorkspaceDirEnv({ [envVar]: FAKE_PATH }, () => findWorkspaceDirSync(process.cwd()))).toBe(FAKE_PATH)
})

test('PNPM_CONFIG_WORKSPACE_DIR takes precedence over NPM_CONFIG_WORKSPACE_DIR', async () => {
  const workspaceDir = await withWorkspaceDirEnv({
    PNPM_CONFIG_WORKSPACE_DIR: FAKE_PATH,
    NPM_CONFIG_WORKSPACE_DIR: 'OTHER_PATH',
  }, () => findWorkspaceDir(process.cwd()))

  expect(workspaceDir).toBe(FAKE_PATH)
})

test('an empty PNPM_CONFIG_WORKSPACE_DIR falls back to NPM_CONFIG_WORKSPACE_DIR', async () => {
  const workspaceDir = await withWorkspaceDirEnv({
    PNPM_CONFIG_WORKSPACE_DIR: '',
    NPM_CONFIG_WORKSPACE_DIR: FAKE_PATH,
  }, () => findWorkspaceDir(process.cwd()))

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

async function withWorkspaceDirEnv<Result> (env: Record<string, string>, fn: () => Result | Promise<Result>): Promise<Result> {
  const oldValues = Object.fromEntries(WORKSPACE_DIR_ENV_VARS.map((name) => [name, process.env[name]]))
  for (const name of WORKSPACE_DIR_ENV_VARS) {
    delete process.env[name]
  }
  Object.assign(process.env, env)
  try {
    return await fn()
  } finally {
    for (const [name, value] of Object.entries(oldValues)) {
      if (value == null) {
        delete process.env[name]
      } else {
        process.env[name] = value
      }
    }
  }
}
