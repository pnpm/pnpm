import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { PackageManifest } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test.each(['constructor', 'toString'])('install tolerates an absent metadata-only optional peer named %s', async (peerName) => {
  const project = prepareEmpty()
  await install({ dependencies: { 'is-positive': '1.0.0' } }, testDefaults({
    autoInstallPeers: false,
    strictPeerDependencies: true,
    hooks: {
      readPackage: [(manifest: PackageManifest) => {
        if (manifest.name === 'is-positive') {
          manifest.peerDependenciesMeta = { [peerName]: { optional: true } }
        }
        return manifest
      }],
    },
  }))
  expect(project.readLockfile().packages['is-positive@1.0.0'].peerDependenciesMeta)
    .toEqual({ [peerName]: { optional: true } })
  expect(project.readLockfile().snapshots['is-positive@1.0.0'].dependencies?.[peerName]).toBeUndefined()
})

test.each(['constructor', 'toString'])('install reports an incompatible peer named %s', async (peerName) => {
  prepareEmpty()
  await expect(install({ dependencies: { 'is-positive': '1.0.0', [peerName]: 'npm:is-negative@1.0.0' } }, testDefaults({
    autoInstallPeers: false,
    strictPeerDependencies: true,
    hooks: {
      readPackage: [(manifest: PackageManifest) => {
        if (manifest.name === 'is-positive') {
          manifest.peerDependencies = { [peerName]: '^2.0.0' }
        }
        return manifest
      }],
    },
  }))).rejects.toMatchObject({
    code: 'ERR_PNPM_PEER_DEP_ISSUES',
    issuesByProjects: {
      '.': {
        bad: { [peerName]: [expect.objectContaining({ foundVersion: '1.0.0', wantedRange: '^2.0.0' })] },
      },
    },
  })
})

test.each(['constructor', 'toString'])('install binds a metadata-only optional peer named %s', async (peerName) => {
  const project = prepareEmpty()
  await install({ dependencies: { 'is-positive': '1.0.0', [peerName]: 'npm:is-negative@1.0.0' } }, testDefaults({
    autoInstallPeers: false,
    strictPeerDependencies: true,
    hooks: {
      readPackage: [(manifest: PackageManifest) => {
        if (manifest.name === 'is-positive') {
          manifest.peerDependenciesMeta = { [peerName]: { optional: true } }
        }
        return manifest
      }],
    },
  }))
  const lockfile = project.readLockfile()
  const consumer = Object.entries(lockfile.snapshots).find(([key]) => key.startsWith('is-positive@'))![1]
  expect(consumer.optionalDependencies).toHaveProperty([peerName], 'is-negative@1.0.0')
})
