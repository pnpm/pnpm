import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// Covers https://github.com/pnpm/pnpm/issues/11155
// autoInstallPeers should NOT install optional peer dependencies.
// Only non-optional (required) peers should be auto-installed.
//
// @pnpm.e2e/abc-optional-peers has:
//   peerDependencies: { peer-a: ^1, peer-b: ^1, peer-c: ^1 }
//   peerDependenciesMeta: { peer-b: { optional: true }, peer-c: { optional: true } }
//
// So peer-a is required, peer-b and peer-c are optional.

test('autoInstallPeers should not install optional peer dependencies', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-optional-peers': '1.0.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    autoInstallPeers: true,
  })
  await execPnpm(['install'])

  const lockfile = readYamlFileSync<LockfileFile>(path.resolve(WANTED_LOCKFILE))
  const project1Deps = lockfile.importers?.['project-1']?.dependencies ?? {}

  // The resolved version string for abc-optional-peers encodes which peers were resolved.
  // peer-a is required — it SHOULD appear in the resolution
  const abcVersion = project1Deps['@pnpm.e2e/abc-optional-peers']?.version ?? ''
  expect(abcVersion).toContain('@pnpm.e2e/peer-a@')

  // peer-b and peer-c are optional (peerDependenciesMeta: { optional: true }) —
  // they should NOT be resolved since no workspace package depends on them.
  // BUG: pnpm currently resolves them anyway.
  expect(abcVersion).not.toContain('@pnpm.e2e/peer-b@')
  expect(abcVersion).not.toContain('@pnpm.e2e/peer-c@')
})

test('autoInstallPeers should not install optional peers even when another workspace package provides one', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-optional-peers': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',

        dependencies: {
          // project-2 uses peer-b directly, but project-1 does not
          '@pnpm.e2e/peer-b': '1.0.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    autoInstallPeers: true,
  })
  await execPnpm(['install'])

  const lockfile = readYamlFileSync<LockfileFile>(path.resolve(WANTED_LOCKFILE))
  const project1Deps = lockfile.importers?.['project-1']?.dependencies ?? {}

  const abcVersion = project1Deps['@pnpm.e2e/abc-optional-peers']?.version ?? ''

  // peer-a is required — should be resolved for project-1
  expect(abcVersion).toContain('@pnpm.e2e/peer-a@')

  // peer-b exists in project-2 but is optional for abc-optional-peers —
  // it should NOT be resolved into project-1's abc-optional-peers just
  // because it exists somewhere in the workspace.
  // BUG: pnpm currently resolves it because it's in allPreferredVersions.
  expect(abcVersion).not.toContain('@pnpm.e2e/peer-b@')

  // peer-c is optional and not depended on by anyone — should not be resolved
  expect(abcVersion).not.toContain('@pnpm.e2e/peer-c@')
})
