import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { getNodeLoaderPaths } from '../lib/nodeLoaderPaths.js'

test('configured lockfile directory owns the loaders, including when they are missing', () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const lockfileDir = path.resolve('lockfiles')
  for (const dir of [workspaceDir, lockfileDir]) {
    fs.mkdirSync(path.join(dir, 'vendor'), { recursive: true })
    fs.writeFileSync(path.join(dir, '.pnp.cjs'), '')
    fs.writeFileSync(path.join(dir, 'vendor/.package-map.json'), '{}')
  }
  const opts = { workspaceDir, lockfileDir, modulesDir: 'vendor', nodeExperimentalPackageMap: true }
  const paths = getNodeLoaderPaths(opts)
  expect(paths.workspacePnpPath).toBe(path.join(lockfileDir, '.pnp.cjs'))
  expect(paths.workspacePackageMapPath).toBe(path.join(lockfileDir, 'vendor/.package-map.json'))
  fs.rmSync(lockfileDir, { recursive: true })
  const missing = getNodeLoaderPaths(opts)
  expect(missing.workspacePnpPath).toBeUndefined()
  expect(missing.workspacePackageMapPath).toBeUndefined()
  expect(missing.existsPnp(workspaceDir)).toBeUndefined()
  expect(missing.existsPackageMap(workspaceDir)).toBeUndefined()
})
