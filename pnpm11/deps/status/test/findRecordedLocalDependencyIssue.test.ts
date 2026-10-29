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
  }), new Set())).toBe('The local dependency at "/repo/vendor/pkg.tgz" may have changed')
})

test('findRecordedLocalDependencyIssue() finds nothing in a state that records no local dependency', () => {
  expect(findRecordedLocalDependencyIssue(state(), new Set())).toBeUndefined()
  expect(findRecordedLocalDependencyIssue(state({ recordedAtNs: 0, manifests: {} }), new Set())).toBeUndefined()
})

test('findRecordedLocalDependencyIssue() reports a lockfile whose local dependencies are not recorded', () => {
  const unrecorded = 'The local dependencies of a lockfile are not recorded'
  expect(findRecordedLocalDependencyIssue(state({ recordedAtNs: 0, manifests: {}, lockfile: 'current' }), new Set())).toBe(unrecorded)
  expect(findRecordedLocalDependencyIssue(state({
    recordedAtNs: 0,
    manifests: {},
    lockfile: 'current',
    lockfiles: { b: 'other' },
    localDependencies: { current: {} },
  }), new Set())).toBe(unrecorded)
  expect(findRecordedLocalDependencyIssue(state({
    recordedAtNs: 0,
    manifests: {},
    lockfile: 'current',
    lockfiles: { b: 'other', c: null },
    localDependencies: { current: {}, other: {} },
  }), new Set())).toBeUndefined()
})

test('findRecordedLocalDependencyIssue() takes a recorded directory that is a workspace project for no local dependency', () => {
  const recorded = state({ recordedAtNs: 0, manifests: {}, localDependencies: { digest: { directories: ['/repo/packages/a'] } } })
  expect(findRecordedLocalDependencyIssue(recorded, new Set(['/repo/packages/a']))).toBeUndefined()
  expect(findRecordedLocalDependencyIssue(recorded, new Set())).toBe('The local dependency at "/repo/packages/a" may have changed')
})
