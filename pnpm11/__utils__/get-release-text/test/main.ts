import { execFile } from 'node:child_process'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

import { afterEach, beforeEach, expect, test } from '@jest/globals'

import { getChangelogEntry, writeReleaseText } from '../src/main.js'

const SPONSORS_FRAGMENT = '<!-- sponsors -->\n\n## Platinum Sponsors\n\n<!-- sponsors end -->\n'

const execFileAsync = promisify(execFile)
const scriptPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../src/main.ts')
// Node strips types on its own from v23.6; the older versions the test matrix
// still covers need the flag to load the TypeScript entry point at all.
const [nodeMajor, nodeMinor] = process.versions.node.split('.').map(Number)
const stripTypes = nodeMajor < 23 || (nodeMajor === 23 && nodeMinor < 6) ? ['--experimental-strip-types'] : []

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

test('appends the shared sponsors fragment exactly once', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))
  const pendingDir = path.join(workspaceDir, '.changeset/changelogs')
  await fs.mkdir(pendingDir, { recursive: true })
  await fs.writeFile(path.join(pendingDir, 'pnpm@11.13.1.md'), '## 11.13.1\n\n### Patch Changes\n\n- Fixed the release notes.\n')
  const githubDir = path.join(workspaceDir, '.github')
  await fs.mkdir(githubDir, { recursive: true })
  await fs.writeFile(path.join(githubDir, 'release-sponsors.md'), SPONSORS_FRAGMENT)

  await writeReleaseText(workspaceDir)

  const release = await fs.readFile(path.join(workspaceDir, 'RELEASE.md'), 'utf8')
  expect(release).toContain('Fixed the release notes.')
  // the fragment is the tail of the description, appended once and only once —
  // a second append, or markup left behind in getChangelogEntry, fails here
  expect(release.match(/<!-- sponsors -->/g)).toHaveLength(1)
  expect(release.endsWith(`\n${SPONSORS_FRAGMENT}`)).toBe(true)
  // separated from the changelog by a blank line
  expect(release).toContain('\n\n<!-- sponsors -->')
})

test('writes the release description when the sponsors fragment is missing', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))
  const pendingDir = path.join(workspaceDir, '.changeset/changelogs')
  await fs.mkdir(pendingDir, { recursive: true })
  await fs.writeFile(path.join(pendingDir, 'pnpm@11.13.1.md'), '## 11.13.1\n\n### Patch Changes\n\n- Fixed the release notes.\n')

  await writeReleaseText(workspaceDir)

  const release = await fs.readFile(path.join(workspaceDir, 'RELEASE.md'), 'utf8')
  expect(release).toContain('Fixed the release notes.')
  expect(release).not.toContain('<!-- sponsors -->')
})

test('writes the release description when run as a script', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))
  const pendingDir = path.join(workspaceDir, '.changeset/changelogs')
  await fs.mkdir(pendingDir, { recursive: true })
  await fs.writeFile(path.join(pendingDir, 'pnpm@11.13.1.md'), '## 11.13.1\n\n### Patch Changes\n\n- Fixed the release notes.\n')

  // the release job runs the module, it doesn't import it; only this path
  // evaluates the top-level entry point
  await execFileAsync(process.execPath, [...stripTypes, scriptPath, workspaceDir])

  const release = await fs.readFile(path.join(workspaceDir, 'RELEASE.md'), 'utf8')
  expect(release).toContain('Fixed the release notes.')
})

test('reports a missing changelog for the released version', async () => {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  await fs.mkdir(pnpmDir, { recursive: true })
  await fs.writeFile(path.join(pnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '11.13.1' }))

  await expect(writeReleaseText(workspaceDir)).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_CHANGELOG',
    message: 'No pending changelog found for pnpm 11.13.1',
  })
})

test('rejects a changelog without the released version', () => {
  let thrown: unknown
  try {
    getChangelogEntry('# pnpm\n\n## 11.13.0\n', '11.13.1')
  } catch (err: unknown) {
    thrown = err
  }
  expect(thrown).toMatchObject({
    code: 'ERR_PNPM_MISSING_CHANGELOG_ENTRY',
    message: 'No changelog entry found for pnpm 11.13.1',
  })
})
