import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { WorkspaceProject } from '@pnpm/releasing.versioning'

import { privateOnlyProjectNames } from '../src/privateProjects.js'

const workspaceDir = path.resolve('/workspace')

function project (dir: string, manifest: { name: string, private?: boolean }): WorkspaceProject {
  return { rootDir: path.join(workspaceDir, dir), manifest: { version: '1.0.0', ...manifest } }
}

test('privateOnlyProjectNames() returns the names only private projects carry', () => {
  expect(privateOnlyProjectNames([
    project('packages/app', { name: 'app', private: true }),
    project('packages/lib', { name: 'lib' }),
  ])).toEqual(new Set(['app']))
})

// Parked changelogs are keyed by manifest name, so skipping a name a public
// project shares would leave that project's release unconfirmed forever.
test('privateOnlyProjectNames() leaves out a name a public project shares', () => {
  expect(privateOnlyProjectNames([
    project('apps/app', { name: 'app', private: true }),
    project('packages/app', { name: 'app' }),
  ])).toEqual(new Set())
})
