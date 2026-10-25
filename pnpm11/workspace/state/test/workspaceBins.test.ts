import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { workspaceBinsDigest } from '../src/index.js'

test('workspace bin digests match between implementations and ignore layout', () => {
  const root = path.resolve('project')
  const digest = (manifest: ProjectManifest, workspaceDir = root): string | undefined => workspaceBinsDigest(workspaceDir, [{
    rootDir: path.join(workspaceDir, 'packages/a') as ProjectRootDir,
    manifest,
  }])
  const original = { name: 'a', bin: { 'a-one': 'one.js', 'a-two': 'two.js' } }
  const expected = '341026c69559cca8573860896362ff03'
  expect(digest(original)).toBe(expected)
  expect(digest({ bin: { 'a-two': 'two.js', 'a-one': 'one.js' }, name: 'a', description: 'changed' })).toBe(expected)
  expect(digest(original, path.resolve('moved'))).toBe(expected)
  const changes: ProjectManifest[] = [
    { name: 'a', bin: { 'a-one': 'two.js', 'a-two': 'two.js' } },
    { name: 'a', bin: { 'a-renamed': 'one.js', 'a-two': 'two.js' } },
    { name: 'a', bin: { 'a-one': 'one.js' } },
    { name: 'a', bin: 'one.js' },
    { name: 'renamed', bin: { 'a-one': 'one.js', 'a-two': 'two.js' } },
    { name: 'a', directories: { bin: 'bin' } },
    { name: 'a' },
  ]
  for (const changed of changes) {
    expect(digest(changed)).not.toBe(expected)
  }
  expect(digest({ name: 'a' })).toBeUndefined()
})
