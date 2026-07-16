import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'

import { getChangelogEntry, writeReleaseText } from '../src/main.js'

let workspaceDir: string

beforeEach(() => {
  workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-release-text-'))
})

afterEach(() => {
  fs.rmSync(workspaceDir, { recursive: true, force: true })
})

test('writes the parked registry changelog section', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  fs.mkdirSync(pnpmDir, { recursive: true })
  fs.writeFileSync(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))
  fs.writeFileSync(path.join(pnpmDir, 'CHANGELOG.md'), '# pnpm\n\n## 11.13.0\n\nOld release.\n')
  const pendingDir = path.join(workspaceDir, '.changeset/changelogs')
  fs.mkdirSync(pendingDir, { recursive: true })
  fs.writeFileSync(path.join(pendingDir, 'pnpm@11.13.1.md'), '## 11.13.1\n\n### Patch Changes\n\n- Fixed the release notes.\n')

  await writeReleaseText(workspaceDir)

  const release = fs.readFileSync(path.join(workspaceDir, 'RELEASE.md'), 'utf8')
  expect(release).toContain('Fixed the release notes.')
  expect(release).not.toContain('Old release.')
})

test('rejects a changelog without the released version', () => {
  expect(() => getChangelogEntry('# pnpm\n\n## 11.13.0\n', '11.13.1')).toThrow('No changelog entry found for pnpm 11.13.1')
})
