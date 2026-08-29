import path from 'node:path'

import { expect, test } from '@jest/globals'

import { getCacheDirectory } from '../config.js'

test('uses the workspace-relative project path for the cache directory', () => {
  const workspaceDir = path.join(import.meta.dirname, '../../../..')
  const cacheCommands = getCacheDirectory(path.join(workspaceDir, 'pnpm11/cache/commands'))
  const registryCommands = getCacheDirectory(path.join(workspaceDir, 'pnpm11/registry-access/commands'))

  expect(cacheCommands).toMatch(/\.jest-cache[/\\]pnpm11[/\\]cache[/\\]commands$/)
  expect(registryCommands).toMatch(/\.jest-cache[/\\]pnpm11[/\\]registry-access[/\\]commands$/)
  expect(cacheCommands).not.toBe(registryCommands)
})
