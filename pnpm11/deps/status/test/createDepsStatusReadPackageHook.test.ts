import path from 'node:path'

import { expect, test } from '@jest/globals'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import type { ProjectManifest } from '@pnpm/types'

import { createDepsStatusReadPackageHook } from '../src/assertWantedLockfileUpToDate.js'
import type { CheckDepsStatusOptions } from '../src/types.js'

test('a relative override path resolves from the workspace root for a project with its own lockfile', async () => {
  const workspaceDir = path.resolve('workspace')
  const projectDir = path.join(workspaceDir, 'packages/deep/consumer')
  const config = { workspaceDir, sharedWorkspaceLockfile: false } as CheckDepsStatusOptions
  const readPackageHook = createDepsStatusReadPackageHook(config, projectDir, parseOverrides({ target: 'file:./packages/target' }, {}))

  const manifest = await readPackageHook!({ name: 'consumer', dependencies: { target: '1.0.0' } } as ProjectManifest, projectDir)

  expect(manifest.dependencies?.target).toBe('file:../../target')
})
