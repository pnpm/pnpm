import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'

import { getChangelogEntry, writeReleaseText } from '../src/main.js'

let workspaceDir: string

beforeEach(async () => {
  workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'get-release-text-'))
})

afterEach(async () => {
  await fs.rm(workspaceDir, { recursive: true, force: true })
})

test('writes the pending registry changelog section', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))
  const pendingDir = path.join(workspaceDir, '.changeset/changelogs')
  await fs.mkdir(pendingDir, { recursive: true })
  await fs.writeFile(path.join(pendingDir, 'pnpm@11.13.1.md'), '## 11.13.1\n\n### Patch Changes\n\n- Fixed the release notes.\n')

  await writeReleaseText(workspaceDir)

  const release = await fs.readFile(path.join(workspaceDir, 'RELEASE.md'), 'utf8')
  expect(release).toContain('Fixed the release notes.')
})

test('reports a missing changelog for the released version', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))

  await expect(writeReleaseText(workspaceDir)).rejects.toThrow('No pending changelog found for pnpm 11.13.1')
})

test('rejects a changelog without the released version', () => {
  expect(() => getChangelogEntry('# pnpm\n\n## 11.13.0\n', '11.13.1')).toThrow('No changelog entry found for pnpm 11.13.1')
})
