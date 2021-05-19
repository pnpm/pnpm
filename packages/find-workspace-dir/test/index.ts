/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import findWorkspaceDir from '@pnpm/find-workspace-dir'

const NPM_CONFIG_WORKSPACE_DIR_ENV_VAR = 'NPM_CONFIG_WORKSPACE_DIR'
const FAKE_PATH = 'FAKE_PATH'

test('finds actual workspace dir', async () => {
  const workspaceDir = await findWorkspaceDir(process.cwd())

  expect(workspaceDir).toBe(path.resolve(__dirname, '..', '..', '..'))
})

test('finds overriden workspace dir', async () => {
  const oldValue = process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR]
  process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = FAKE_PATH
  const workspaceDir = await findWorkspaceDir(process.cwd())
  process.env[NPM_CONFIG_WORKSPACE_DIR_ENV_VAR] = oldValue

  expect(workspaceDir).toBe(FAKE_PATH)
})
