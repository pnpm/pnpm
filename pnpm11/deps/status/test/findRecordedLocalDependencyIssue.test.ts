import { expect, test } from '@jest/globals'
import type { WorkspaceState } from '@pnpm/workspace.state'

import { findRecordedLocalDependencyIssue } from '../src/findLocalFileDepsIssue.js'

const state = (validation?: WorkspaceState['validation']): WorkspaceState => ({
  lastValidatedTimestamp: 0,
  projects: {},
  pnpmfiles: [],
  filteredInstall: false,
  settings: { excludeLinksFromLockfile: false, linkWorkspacePackages: false, preferWorkspacePackages: false },
  validation,
})

test('findRecordedLocalDependencyIssue() names a local dependency the last install recorded', () => {
  expect(findRecordedLocalDependencyIssue(state({
    recordedAtNs: 0,
    manifests: {},
    localDependencies: { digest: { tarballs: ['/repo/vendor/pkg.tgz'] } },
  }))).toBe('The local dependency at "/repo/vendor/pkg.tgz" may have changed')
})

test('findRecordedLocalDependencyIssue() finds nothing in a state that records no local dependency', () => {
  expect(findRecordedLocalDependencyIssue(state())).toBeUndefined()
  expect(findRecordedLocalDependencyIssue(state({ recordedAtNs: 0, manifests: {} }))).toBeUndefined()
})
