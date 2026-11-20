import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/testing.registry-mock'
import type { ProjectManifest } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

import { testDefaults } from '../utils/index.js'

// Every test locks two versions of @pnpm.e2e/dep-of-pkg-with-1-dep, both in
// the range of the edge under test, and pins that edge to the lower one. A
// resolution that drops the pin moves the edge to the higher one.

test('adding a dependency keeps the locked version of an npm-aliased subdependency', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/parent-of-pkg-with-1-dep': '1.0.0',
      '@pnpm.e2e/pkg-with-1-aliased-dep': '100.0.0',
    },
  }
  writeLockfile({
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/parent-of-pkg-with-1-dep': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/pkg-with-1-aliased-dep': { specifier: '100.0.0', version: '100.0.0' },
        },
      },
    },
    packages: {
      ...lockedPackages(),
      '@pnpm.e2e/parent-of-pkg-with-1-dep@1.0.0': {
        resolution: { integrity: getIntegrity('@pnpm.e2e/parent-of-pkg-with-1-dep', '1.0.0') },
      },
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        resolution: { integrity: getIntegrity('@pnpm.e2e/pkg-with-1-aliased-dep', '100.0.0') },
      },
    },
    snapshots: {
      ...lockedSnapshots({ childOfPkgWith1Dep: '100.1.0' }),
      '@pnpm.e2e/parent-of-pkg-with-1-dep@1.0.0': {
        dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
      },
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        dependencies: { dep: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' },
      },
    },
  })

  await addDependenciesToPackage(manifest, ['is-positive@1.0.0'], testDefaults({ lockfileOnly: true }))

  expect(project.readLockfile().snapshots['@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0']).toStrictEqual({
    dependencies: { dep: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' },
  })
})

test('adding a dependency keeps the locked dependencies of an auto-installed peer', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/has-pkg-with-1-dep-peer': '1.0.0',
      '@pnpm.e2e/pkg-with-1-aliased-dep': '100.0.0',
    },
  }
  writeLockfile({
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/has-pkg-with-1-dep-peer': { specifier: '1.0.0', version: '1.0.0(@pnpm.e2e/pkg-with-1-dep@100.0.0)' },
          '@pnpm.e2e/pkg-with-1-aliased-dep': { specifier: '100.0.0', version: '100.0.0' },
        },
      },
    },
    packages: {
      ...lockedPackages(),
      '@pnpm.e2e/has-pkg-with-1-dep-peer@1.0.0': {
        resolution: { integrity: getIntegrity('@pnpm.e2e/has-pkg-with-1-dep-peer', '1.0.0') },
        peerDependencies: { '@pnpm.e2e/pkg-with-1-dep': '^100.0.0' },
      },
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        resolution: { integrity: getIntegrity('@pnpm.e2e/pkg-with-1-aliased-dep', '100.0.0') },
      },
    },
    snapshots: {
      ...lockedSnapshots({ childOfPkgWith1Dep: '100.0.0' }),
      '@pnpm.e2e/has-pkg-with-1-dep-peer@1.0.0(@pnpm.e2e/pkg-with-1-dep@100.0.0)': {
        dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
      },
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        dependencies: { dep: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' },
      },
    },
  })

  await addDependenciesToPackage(manifest, ['is-positive@1.0.0'], testDefaults({ lockfileOnly: true }))

  expect(project.readLockfile().snapshots['@pnpm.e2e/pkg-with-1-dep@100.0.0']).toStrictEqual({
    dependencies: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0' },
  })
})

function writeLockfile (lockfile: {
  importers: Record<string, unknown>
  packages: Record<string, unknown>
  snapshots: Record<string, unknown>
}): void {
  writeYamlFileSync(WANTED_LOCKFILE, {
    lockfileVersion: LOCKFILE_VERSION,
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    ...lockfile,
  }, { lineWidth: 1000 })
}

function lockedPackages (): Record<string, unknown> {
  return {
    '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': {
      resolution: { integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0') },
    },
    '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
      resolution: { integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0') },
    },
    '@pnpm.e2e/pkg-with-1-dep@100.0.0': {
      resolution: { integrity: getIntegrity('@pnpm.e2e/pkg-with-1-dep', '100.0.0') },
    },
  }
}

function lockedSnapshots (opts: { childOfPkgWith1Dep: string }): Record<string, unknown> {
  return {
    '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': {},
    '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {},
    '@pnpm.e2e/pkg-with-1-dep@100.0.0': {
      dependencies: { '@pnpm.e2e/dep-of-pkg-with-1-dep': opts.childOfPkgWith1Dep },
    },
  }
}
