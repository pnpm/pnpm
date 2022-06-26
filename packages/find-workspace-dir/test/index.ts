/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import fs from 'fs'
import findWorkspaceDir from '@pnpm/find-workspace-dir'

const NPM_CONFIG_WORKSPACE_DIR_ENV_VAR = 'NPM_CONFIG_WORKSPACE_DIR'
const FAKE_PATH = 'FAKE_PATH'

function isFileSystemCaseSensitive () {
  try {
    fs.realpathSync.native(process.cwd().toUpperCase())
    return false
  } catch (_) {
    return true
  }
}

// We don't need to validate case-sensitive systems
// because it is not possible to reach process.cwd() with wrong case there.
const testOnCaseInSensitiveSystems = isFileSystemCaseSensitive() ? test.skip : test

test('finds actual workspace dir', async () => {
  const workspaceDir = await findWorkspaceDir(process.cwd())

  expect(workspaceDir).toBe(path.resolve(__dirname, '..', '..', '..'))
})

testOnCaseInSensitiveSystems('finds workspace dir with wrong case from cwd', async () => {
  const workspaceDir = await findWorkspaceDir(process.cwd().toUpperCase())

  expect(workspaceDir).toBe(path.resolve(__dirname, '..', '..', '..'))
})

test('finds overriden workspace dir', async () => {
  const oldValue = process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR]
  process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = FAKE_PATH
  const workspaceDir = await findWorkspaceDir(process.cwd())
  process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = oldValue

  expect(workspaceDir).toBe(FAKE_PATH)
})
