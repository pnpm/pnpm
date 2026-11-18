import { expect, test } from '@jest/globals'

import { readLockedGitPackage } from '../lib/lockedGitPackage.js'
import type { DependencyRequest } from '../lib/requestDependencyPackage.js'

test('locked Git manifests use the locked artifact specifiers and strip peer suffixes', () => {
  const request = lockedGitRequest()
  expect(readLockedGitPackage(request)?.body.manifest?.dependencies).toStrictEqual({
    'git-alias': 'git+https://example.test/child.git#0123456789012345678901234567890123456789',
    'registry-alias': 'npm:real-name@1.0.0',
    'named-alias': 'mirror:real-name@2.0.0',
    'named-child': 'mirror:3.0.0',
    'local-child': 'link:../local-child',
  })
})

test('locked Git manifests are not reused when the importer changes its selector', () => {
  const request = lockedGitRequest()
  request.wantedDependency.bareSpecifier = 'git+https://example.test/parent.git#other'
  expect(readLockedGitPackage(request)).toBeUndefined()
})

test('locked Git manifests are only reused for trusted resolution without fetching', () => {
  const request = lockedGitRequest()
  request.ctx.dryRun = false
  expect(readLockedGitPackage(request)).toBeUndefined()
  request.ctx.dryRun = true
  request.ctx.trustLockfile = false
  expect(readLockedGitPackage(request)).toBeUndefined()
})

function lockedGitRequest (): DependencyRequest {
  const parentRepo = 'https://example.test/parent.git'
  const commit = '0123456789012345678901234567890123456789'
  const bareSpecifier = `git+${parentRepo}#main`
  const resolution = { type: 'git', repo: parentRepo, commit }
  return {
    ctx: {
      dryRun: true,
      trustLockfile: true,
      lockedPeersAreCurrent: true,
      lockfileDir: '/project',
      wantedLockfile: { importers: { '.': { specifiers: { parent: bareSpecifier } } } },
    },
    currentPkg: {
      name: 'parent',
      version: '1.0.0',
      pkgId: `git+${parentRepo}#${commit}`,
      resolution,
      dependencyLockfile: {
        resolution,
        dependencies: {
          'git-alias': `child@git+https://example.test/child.git#${commit}(peer@1.0.0)`,
          'registry-alias': 'real-name@1.0.0(peer@1.0.0)',
          'named-alias': 'real-name@mirror:2.0.0(peer@1.0.0)',
          'named-child': 'mirror:3.0.0(peer@1.0.0)',
          'local-child': 'link:../local-child',
        },
      },
    },
    options: { currentDepth: 0, updateDepth: -1, parentPkg: { rootDir: '/project' } },
    wantedDependency: { alias: 'parent', bareSpecifier },
  } as unknown as DependencyRequest
}
