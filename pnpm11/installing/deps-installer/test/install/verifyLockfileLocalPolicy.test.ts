import { expect, jest, test } from '@jest/globals'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/store.controller-types'
import type { BaseManifest, DepPath, ProjectId, ReadPackageHook } from '@pnpm/types'

import {
  appendPristineDeclaredSpecs,
  appendProbedHookEdges,
  pristineSpecMapKey,
} from '../../src/install/verifyLockfileLocalPolicy.js'
import {
  createLocalResolutionPolicy,
  type LocalHookInputs,
  resolveLocalPolicyForInstall,
} from '../../src/install/verifyLockfileLocalResolutions.js'
import {
  verifyLockfileFileOffline,
  verifyLockfileLocalResolutions,
  verifyLockfileResolutions,
  verifyLockfileResolutionShapes,
} from '../../src/install/verifyLockfileResolutions.js'
import { canonicalSlashPath } from '../../src/install/verifyLockfileShape.js'

function buildPolicy (inputs: LocalHookInputs) {
  return createLocalResolutionPolicy(inputs)
}

test('bare Windows drive, case-insensitive drive, and cross-volume anchored overrides match while drive-relative is rejected', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:../helper' },
      },
      ['helper@file:../helper' as DepPath]: {
        resolution: { type: 'directory', directory: '../helper' },
      },
    },
  }
  for (const spec of ['file:C:/work/helper', 'C:/work/helper', 'C:\\work\\helper', 'file:c:/WORK/app/../helper']) {
    const localPolicy = buildPolicy({
      lockfileDir: 'C:/work/app',
      overrides: { 'remote-pkg>helper': spec },
    })
    // eslint-disable-next-line no-await-in-loop -- sequential assertion per spec variant
    await expect(verifyLockfileResolutions(lockfile, [], { localPolicy })).resolves.toBeUndefined()
  }

  const crossDrive = buildPolicy({
    lockfileDir: 'C:/work/app',
    overridesRootDir: 'D:/workspace',
    overrides: { 'remote-pkg>helper': 'file:C:/work/helper' },
  })
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: crossDrive })).resolves.toBeUndefined()

  const driveRelative = buildPolicy({
    lockfileDir: 'C:/work/app',
    overrides: { 'remote-pkg>helper': 'C:helper' },
  })
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: driveRelative })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('UNC volume roots rebase only within the same server and share', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:../helper' },
      },
      ['helper@file:../helper' as DepPath]: {
        resolution: { type: 'directory', directory: '../helper' },
      },
    },
  }
  const sameShare = buildPolicy({
    lockfileDir: '//srv1/share1/app',
    overrides: { 'remote-pkg>helper': 'file://SRV1/SHARE1/helper' },
  })
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: sameShare })).resolves.toBeUndefined()

  for (const crossShare of ['file://srv1/share2/helper', 'file://srv2/share1/helper']) {
    const crossPolicy = buildPolicy({
      lockfileDir: '//srv1/share1/app',
      overrides: { 'remote-pkg>helper': crossShare },
    })
    // eslint-disable-next-line no-await-in-loop -- sequential assertion per cross-share variant
    await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: crossPolicy })).rejects.toMatchObject({
      code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    })
  }
})

test('override target matching rejects reverse joins and cross-importer subpaths', async () => {
  const reverseLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
      ['packages/app' as ProjectId]: { specifiers: {} },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:../shared' },
      },
      ['helper@file:../shared' as DepPath]: {
        resolution: { type: 'directory', directory: '../shared' },
      },
    },
  }
  const reversePolicy = buildPolicy({
    liveWorkspaceDirs: new Set(['.', 'packages/app']),
    overrides: { 'remote-pkg>helper': 'file:../../../shared' },
  })
  await expect(verifyLockfileResolutions(reverseLockfile, [], { localPolicy: reversePolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const crossImporterLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
      ['packages/other' as ProjectId]: { specifiers: {} },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:packages/other/helper' },
      },
      ['helper@file:packages/other/helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'packages/other/helper' },
      },
    },
  }
  const crossPolicy = buildPolicy({
    liveWorkspaceDirs: new Set(['.', 'packages/other']),
    overrides: { 'remote-pkg>helper': 'file:./helper' },
  })
  await expect(verifyLockfileResolutions(crossImporterLockfile, [], { localPolicy: crossPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('reachable policy producer authorizes same-importer peer while unreachable producer and cross-parent link do not', async () => {
  const reachableLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          'safe-parent': '1.0.0',
          'peer-consumer': '1.0.0(helper@file:../private)',
        },
        specifiers: {
          'safe-parent': '1.0.0',
          'peer-consumer': '1.0.0',
        },
      },
    },
    packages: {
      ['safe-parent@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-safe' },
        dependencies: { helper: 'file:../private' },
      },
      ['peer-consumer@1.0.0(helper@file:../private)' as DepPath]: {
        resolution: { integrity: 'sha512-peer' },
        peerDependencies: { helper: '*' },
        dependencies: { helper: 'file:../private' },
      },
      ['helper@file:../private' as DepPath]: {
        resolution: { type: 'directory', directory: '../private' },
      },
    },
  }
  const pkgPolicy = buildPolicy({
    overrides: { 'safe-parent>helper': 'file:../private' },
  })
  await expect(verifyLockfileResolutions(reachableLockfile, [], { localPolicy: pkgPolicy })).resolves.toBeUndefined()

  const unreachableLockfile: LockfileObject = {
    ...reachableLockfile,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'peer-consumer': '1.0.0(helper@file:../private)' },
        specifiers: { 'peer-consumer': '1.0.0' },
      },
    },
  }
  await expect(verifyLockfileResolutions(unreachableLockfile, [], { localPolicy: pkgPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const linkPolicy = buildPolicy({
    overrides: { 'safe-parent>secret': 'link:../private' },
  })
  const safeLinkOnly: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'safe-parent': '1.0.0' },
        specifiers: { 'safe-parent': '1.0.0' },
      },
    },
    packages: {
      ['safe-parent@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-safe' },
        dependencies: { secret: 'link:../private' },
      },
    },
  }
  await expect(verifyLockfileResolutions(safeLinkOnly, [], { localPolicy: linkPolicy })).resolves.toBeUndefined()

  const linkLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'safe-parent': '1.0.0', 'peer-consumer': '1.0.0' },
        specifiers: { 'safe-parent': '1.0.0', 'peer-consumer': '1.0.0' },
      },
    },
    packages: {
      ['safe-parent@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-safe' },
        dependencies: { secret: 'link:../private' },
      },
      ['peer-consumer@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-peer' },
        peerDependencies: { secret: '*' },
        dependencies: { secret: 'link:../private' },
      },
    },
  }
  await expect(verifyLockfileResolutions(linkLockfile, [], { localPolicy: linkPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    message: expect.stringContaining('peer-consumer@1.0.0'),
  })
})

test('resolveLocalPolicyForInstall rejects shape mismatches before store probing, isolates registry fallback, and guards synthetic sentinels', async () => {
  const fetchPackage = jest.fn(async () => ({ fetching: async () => ({ files: { filesMap: new Map(), resolvedFrom: 'local-dir' as const } }), filesIndexFile: '/nonexistent' }))
  const requestPackage = jest.fn(async () => {
    throw new Error('requestPackage must not be called')
  })
  const spyStore = { fetchPackage, requestPackage } as unknown as StoreController
  const helperSpecsSeenByHook: Array<string | undefined> = []
  const sentinelHook: ReadPackageHook = <Pkg extends BaseManifest>(pkg: Pkg): Pkg => {
    helperSpecsSeenByHook.push(pkg.dependencies?.helper)
    if (pkg.dependencies?.helper === '0.0.0') {
      pkg.dependencies = { ...pkg.dependencies, helper: 'file:./local-helper' }
    }
    return pkg
  }

  const shapeTampered: LockfileObject = {
    lockfileVersion: '9.0',
    pnpmfileChecksum: 'sha256-hook',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { type: 'directory', directory: '/etc' },
        dependencies: { helper: 'file:./local-helper' },
      },
      ['helper@file:./local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: './local-helper' },
      },
    },
  }
  await expect(resolveLocalPolicyForInstall(shapeTampered, {
    hooks: { readPackage: [sentinelHook] },
    storeController: spyStore,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
  expect(fetchPackage).not.toHaveBeenCalled()
  expect(requestPackage).not.toHaveBeenCalled()

  // URL-tarball parent with `version: '1.0.0'` must NOT query `requestPackage` for registry `tarball-parent@1.0.0`,
  // and sentinel-conditional hook (`=== '0.0.0'`) must NOT authorize synthetic probe without pristine manifest.
  const tarballParentLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    pnpmfileChecksum: 'sha256-hook',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'tarball-parent': 'https://example.com/parent.tgz' },
        specifiers: { 'tarball-parent': 'https://example.com/parent.tgz' },
      },
    },
    packages: {
      ['tarball-parent@https://example.com/parent.tgz' as DepPath]: {
        version: '1.0.0',
        resolution: { tarball: 'https://example.com/parent.tgz' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  const tarballPolicy = await resolveLocalPolicyForInstall(tarballParentLockfile, {
    hooks: { readPackage: [sentinelHook] },
    overrides: { 'tarball-parent>helper@^1.0.0': 'file:./local-helper' },
    storeController: spyStore,
  })
  expect(requestPackage).not.toHaveBeenCalled()
  expect(helperSpecsSeenByHook).toEqual(expect.arrayContaining(['0.0.0', '0.0.0-pnpm-probe']))
  await expect(verifyLockfileResolutions(tarballParentLockfile, [], { localPolicy: tarballPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('enforces local-git containment, forged importer specifiers, catalog ranges, and per-field / per-baseKey isolation', async () => {
  const remoteToGit: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { 'git-sub': 'git+file:../repo#0123456789abcdef0123456789abcdef01234567' },
      },
      ['git-sub@git+file:../repo#0123456789abcdef0123456789abcdef01234567' as DepPath]: {
        resolution: { type: 'git', repo: 'file:../repo', commit: '0123456789abcdef0123456789abcdef01234567' },
      },
    },
  }
  await expect(verifyLockfileResolutions(remoteToGit, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
  await expect(verifyLockfileResolutions(remoteToGit, [], {
    localPolicy: buildPolicy({ overrides: { 'remote-pkg>git-sub': 'git+file:../repo' } }),
  })).resolves.toBeUndefined()

  const forgedImporter: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': 'file:../private' },
        specifiers: { 'remote-pkg': '^1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@file:../private' as DepPath]: {
        resolution: { type: 'directory', directory: '../private' },
      },
    },
  }
  await expect(verifyLockfileResolutions(forgedImporter, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const dualSourceLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'dup-pkg': '1.0.0', 'evil-dup': 'dup-pkg@https://evil.example/dup.tgz' },
        specifiers: { 'dup-pkg': '1.0.0', 'evil-dup': 'https://evil.example/dup.tgz' },
      },
    },
    packages: {
      ['dup-pkg@1.0.0' as DepPath]: {
        version: '1.0.0',
        resolution: { integrity: 'sha512-good' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['dup-pkg@https://evil.example/dup.tgz' as DepPath]: {
        version: '1.0.0',
        resolution: { tarball: 'https://evil.example/dup.tgz' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  const sourceBoundPolicy = appendProbedHookEdges(buildPolicy({}), [{
    parentBaseKey: 'dup-pkg@1.0.0',
    parentName: 'dup-pkg',
    parentRange: '1.0.0',
    childAlias: 'helper',
    spec: 'file:./local-helper',
  }])
  const registryOnlyLockfile: LockfileObject = {
    ...dualSourceLockfile,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'dup-pkg': '1.0.0' },
        specifiers: { 'dup-pkg': '1.0.0' },
      },
    },
    packages: Object.fromEntries(
      Object.entries(dualSourceLockfile.packages!).filter(([depPath]) => !depPath.includes('evil.example'))
    ) as LockfileObject['packages'],
  }
  await expect(verifyLockfileResolutions(registryOnlyLockfile, [], { localPolicy: sourceBoundPolicy })).resolves.toBeUndefined()
  await expect(verifyLockfileResolutions(dualSourceLockfile, [], { localPolicy: sourceBoundPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    message: expect.stringMatching(/^1 lockfile entries failed verification:/),
  })

  const catalogLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  const wrongFieldPolicy = appendPristineDeclaredSpecs(
    buildPolicy({ overrides: { 'remote-pkg>helper@^1.0.0': 'file:./local-helper' } }),
    new Map([
      [pristineSpecMapKey('remote-pkg@1.0.0', 'optionalDependencies'), { helper: '^1.0.0' }],
      [pristineSpecMapKey('remote-pkg@1.0.0', 'dependencies'), { helper: '^2.0.0' }],
    ])
  )
  await expect(verifyLockfileResolutions(catalogLockfile, [], { localPolicy: wrongFieldPolicy })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const catalogPolicy = appendPristineDeclaredSpecs(
    buildPolicy({
      catalogs: { default: { helper: '^1.0.0' } },
      overrides: { 'remote-pkg>helper@^1.0.0': 'file:./local-helper' },
    }),
    new Map([[pristineSpecMapKey('remote-pkg@1.0.0', 'dependencies'), { helper: 'catalog:' }]])
  )
  await expect(verifyLockfileResolutions(catalogLockfile, [], { localPolicy: catalogPolicy })).resolves.toBeUndefined()
})

test('an importer range linked to a lockfile importer absent from the run passes', async () => {
  const lockfile = (withImporter: boolean): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { lib: 'link:../shared' },
        specifiers: { lib: '^1.0.0' },
      },
      ...(withImporter ? { ['../shared' as ProjectId]: { specifiers: {} } } : {}),
    },
  })
  const localPolicy = () => buildPolicy({ liveWorkspaceDirs: new Set(['.']) })
  await expect(verifyLockfileResolutions(lockfile(true), [], { localPolicy: localPolicy() })).resolves.toBeUndefined()
  await expect(verifyLockfileResolutions(lockfile(false), [], { localPolicy: localPolicy() })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('an override target does not match a path that steps back through a segment', async () => {
  const lockfile = (target: string): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: `link:${target}` },
      },
    },
  })
  const policy = () => buildPolicy({ overrides: { 'remote-pkg>helper': 'link:./shared' } })
  await expect(verifyLockfileResolutions(lockfile('shared'), [], { localPolicy: policy() })).resolves.toBeUndefined()
  await expect(verifyLockfileResolutions(lockfile('linked/../shared'), [], { localPolicy: policy() })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('a non-local peer matching the peer of a reachable local package passes', async () => {
  const lockfile = (localLibIsDirect: boolean): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { remote: '1.0.0', ...(localLibIsDirect ? { 'local-lib': 'file:local-lib' } : {}) },
        specifiers: { remote: '1.0.0', ...(localLibIsDirect ? { 'local-lib': 'file:./local-lib' } : {}) },
      },
    },
    packages: {
      ['helper@file:helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'helper' },
      },
      ['local-lib@file:local-lib' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-lib' },
        dependencies: { helper: 'file:helper' },
        peerDependencies: { helper: 'file:./helper' },
      },
      ['remote@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:helper' },
        peerDependencies: { helper: 'file:./helper' },
      },
    },
  })
  await expect(verifyLockfileResolutions(lockfile(true), [], { localPolicy: buildPolicy({}) })).resolves.toBeUndefined()
  await expect(verifyLockfileResolutions(lockfile(false), [], { localPolicy: buildPolicy({}) })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('an unresolved non-local local peer declaration passes', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { remote: '1.0.0' },
        specifiers: { remote: '1.0.0' },
      },
    },
    packages: {
      ['remote@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        peerDependencies: { helper: 'file:../helper' },
      },
    },
  }
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: buildPolicy({}) })).resolves.toBeUndefined()
})

test('a file://localhost/ git key matches its file://localhost/ repository', async () => {
  const depPath = 'repo@git+file://localhost/tmp/repo#0123456789abcdef0123456789abcdef01234567' as DepPath
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { repo: 'git+file://localhost/tmp/repo#0123456789abcdef0123456789abcdef01234567' },
        specifiers: { repo: 'git+file://localhost/tmp/repo' },
      },
    },
    packages: {
      [depPath]: {
        resolution: { type: 'git', repo: 'git+file://localhost/tmp/repo', commit: '0123456789abcdef0123456789abcdef01234567' },
      },
    },
  }
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: buildPolicy({ lockfileDir: '/work/app' }) })).resolves.toBeUndefined()
})

test('a drive-relative path stays distinct from the anchored path', () => {
  expect(canonicalSlashPath('c:/allowed/helper')).toBe('C:/allowed/helper')
  expect(canonicalSlashPath('C:allowed/helper')).toBe('C:allowed/helper')
  expect(canonicalSlashPath('C:../helper')).toBe('C:../helper')
})

test('a peer resolved only for an unselected importer does not count', () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['app-a' as ProjectId]: {
        dependencies: { plugin: '1.0.0' },
        specifiers: { plugin: '1.0.0' },
      },
      ['app-b' as ProjectId]: {
        dependencies: { plugin: '1.0.0(helper@file:helper)' },
        specifiers: { plugin: '1.0.0' },
      },
    },
    packages: {
      ['plugin@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        peerDependencies: { helper: 'file:../helper' },
      },
      ['plugin@1.0.0(helper@file:helper)' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:helper' },
        peerDependencies: { helper: 'file:../helper' },
      },
      ['helper@file:helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'helper' },
      },
    },
  }
  expect(() => verifyLockfileLocalResolutions(lockfile, buildPolicy({ selectedImporterIds: new Set(['app-a']) }))).not.toThrow()
  expect(() => verifyLockfileLocalResolutions(lockfile, buildPolicy({}))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  }))
})

test('a ranged override covers a peer-only declaration', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { plugin: '1.0.0(helper@helper)' },
        specifiers: { plugin: '1.0.0' },
      },
    },
    packages: {
      ['plugin@1.0.0(helper@helper)' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'link:helper' },
        peerDependencies: { helper: '^1.0.0' },
      },
    },
  }
  const policy = (peerRange: string) => appendPristineDeclaredSpecs(
    buildPolicy({ overrides: { 'plugin>helper@^1.0.0': 'link:./helper' } }),
    new Map([[pristineSpecMapKey('plugin@1.0.0', 'peerDependencies'), { helper: peerRange }]])
  )
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: policy('^1.0.0') })).resolves.toBeUndefined()
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: policy('^2.0.0') })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('an importer link without a specifier is not trusted on its own', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { helper: 'link:../private' },
        specifiers: {},
      },
    },
  }
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: buildPolicy({}) })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

const posixOnly = process.platform === 'win32' ? test.skip : test

posixOnly('the raw lockfile shape check resolves a file URL key against the lockfile directory', () => {
  const lockfile = (directory: string): LockfileFile => ({
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      'helper@file:///work/helper': {
        resolution: { type: 'directory', directory },
      },
    },
  })
  expect(() => verifyLockfileFileOffline(lockfile('../helper'), { lockfileDir: '/work/app' })).not.toThrow()
  expect(() => verifyLockfileFileOffline(lockfile('../other'), { lockfileDir: '/work/app' })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  }))
})

test('a file key backed by a remote tarball or git resolution is a shape mismatch', () => {
  const lockfile = (resolution: Record<string, unknown>): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { disguised: 'file:disguised' },
        specifiers: { disguised: 'file:./disguised' },
      },
    },
    packages: {
      ['disguised@file:disguised' as DepPath]: { resolution: resolution as never },
    },
  })
  for (const resolution of [
    { tarball: 'https://example.com/disguised.tgz' },
    { type: 'git', repo: 'https://example.com/disguised.git', commit: '0123456789abcdef0123456789abcdef01234567' },
  ]) {
    expect(() => verifyLockfileResolutionShapes(lockfile(resolution))).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    }))
  }
  expect(() => verifyLockfileResolutionShapes(lockfile({ type: 'directory', directory: 'disguised' }))).not.toThrow()
})

test('a readPackage hook that throws on a real parent manifest surfaces its own error', async () => {
  const fetchPackage = jest.fn(async () => ({
    fetching: async () => ({
      files: { filesMap: new Map(), resolvedFrom: 'remote' as const },
      bundledManifest: { name: 'tarball-parent', version: '1.0.0', dependencies: { helper: '^1.0.0' } },
    }),
    filesIndexFile: '/nonexistent',
  }))
  const store = { fetchPackage } as unknown as StoreController
  const throwingHook: ReadPackageHook = () => {
    throw new Error('hook exploded')
  }
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    pnpmfileChecksum: 'sha256-hook',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'tarball-parent': 'https://example.com/parent.tgz' },
        specifiers: { 'tarball-parent': 'https://example.com/parent.tgz' },
      },
    },
    packages: {
      ['tarball-parent@https://example.com/parent.tgz' as DepPath]: {
        version: '1.0.0',
        resolution: { tarball: 'https://example.com/parent.tgz' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  await expect(resolveLocalPolicyForInstall(lockfile, {
    hooks: { readPackage: [throwingHook] },
    storeController: store,
  })).rejects.toThrow('hook exploded')
})

test('a hook rewrite keyed on a packageExtensions range authorizes nothing without the real manifest', async () => {
  const redirectExtendedHelper: ReadPackageHook = (pkg) => {
    if (pkg.dependencies?.helper === '^1.0.0') pkg.dependencies.helper = 'file:local-helper'
    return pkg
  }
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    pnpmfileChecksum: 'sha256-hook',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'tarball-parent': 'https://example.com/parent.tgz' },
        specifiers: { 'tarball-parent': 'https://example.com/parent.tgz' },
      },
    },
    packages: {
      ['tarball-parent@https://example.com/parent.tgz' as DepPath]: {
        version: '1.0.0',
        resolution: { tarball: 'https://example.com/parent.tgz' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  // No store controller, so the parent's real manifest is unavailable and the
  // replay cannot tell whether it declares `helper` itself.
  const policy = await resolveLocalPolicyForInstall(lockfile, {
    hooks: { readPackage: [redirectExtendedHelper] },
    lockfileDir: '/work/app',
    packageExtensions: { 'tarball-parent': { dependencies: { helper: '^1.0.0' } } },
  })
  expect(() => verifyLockfileLocalResolutions(lockfile, policy)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  }))
})

test('the hook replay sees in-archive file: dependencies as package-root links', async () => {
  const fetchPackage = jest.fn(async () => ({
    fetching: async () => ({
      files: { filesMap: new Map(), resolvedFrom: 'remote' as const },
      bundledManifest: { name: 'tarball-parent', version: '1.0.0', dependencies: { sub: 'file:./sub', helper: '^1.0.0' } },
    }),
    filesIndexFile: '/nonexistent',
  }))
  const store = { fetchPackage } as unknown as StoreController
  const subSpecsSeenByHook: Array<string | undefined> = []
  const dirsSeenByHook: Array<string | undefined> = []
  const rejectsRawFileSpecs: ReadPackageHook = (pkg, dir) => {
    subSpecsSeenByHook.push(pkg.dependencies?.sub)
    dirsSeenByHook.push(dir)
    if (Object.values(pkg.dependencies ?? {}).some((spec) => spec.startsWith('file:'))) {
      throw new Error('raw file: specifier')
    }
    return pkg
  }
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    pnpmfileChecksum: 'sha256-hook',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'tarball-parent': 'https://example.com/parent.tgz' },
        specifiers: { 'tarball-parent': 'https://example.com/parent.tgz' },
      },
    },
    packages: {
      ['tarball-parent@https://example.com/parent.tgz' as DepPath]: {
        version: '1.0.0',
        resolution: { tarball: 'https://example.com/parent.tgz' },
        dependencies: { helper: 'file:local-helper' },
      },
      ['helper@file:local-helper' as DepPath]: {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    },
  }
  await expect(resolveLocalPolicyForInstall(lockfile, {
    hooks: { readPackage: [rejectsRawFileSpecs] },
    lockfileDir: '/work/app',
    storeController: store,
  })).resolves.toBeDefined()
  expect(subSpecsSeenByHook).toContain('link:<root>/sub')
  expect(new Set(dirsSeenByHook)).toStrictEqual(new Set([undefined]))
})

test('a regular dependency named like a local peer is not a resolved peer', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { plugin: '1.0.0' },
        specifiers: { plugin: '1.0.0' },
      },
    },
    packages: {
      ['plugin@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: '1.0.0' },
        peerDependencies: { helper: 'file:../unused' },
      },
      ['helper@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-helper' },
      },
    },
  }
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: buildPolicy({}) })).resolves.toBeUndefined()
})

test('a file URL target does not match a path that steps back through a segment', async () => {
  const lockfile = (target: string): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'remote-pkg': '1.0.0' },
        specifiers: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      ['remote-pkg@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: `link:${target}` },
      },
    },
  })
  const policy = () => buildPolicy({ overrides: { 'remote-pkg>helper': 'link:/work/app/shared' } })
  await expect(verifyLockfileResolutions(lockfile('///work/app/shared'), [], { localPolicy: policy() })).resolves.toBeUndefined()
  for (const steppingBack of ['///work/app/linked/../shared', '///work/app/linked/%2e%2e/shared']) {
    // eslint-disable-next-line no-await-in-loop -- sequential assertion per target spelling
    await expect(verifyLockfileResolutions(lockfile(steppingBack), [], { localPolicy: policy() })).rejects.toMatchObject({
      code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    })
  }
})

test('an empty package version does not skip the shape check', () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { remote: '1.0.0' },
        specifiers: { remote: '1.0.0' },
      },
    },
    packages: {
      ['remote@1.0.0' as DepPath]: {
        version: '',
        resolution: { type: 'directory', directory: '../outside' },
      },
    },
  }
  expect(() => verifyLockfileResolutionShapes(lockfile)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  }))
})

test('a pristine spec lookup ignores inherited properties such as constructor', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { plugin: '1.0.0(constructor@c)' },
        specifiers: { plugin: '1.0.0' },
      },
    },
    packages: {
      ['plugin@1.0.0(constructor@c)' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { constructor: 'link:c' },
        peerDependencies: { constructor: '^1.0.0' },
      },
    },
  }
  const policy = appendPristineDeclaredSpecs(
    buildPolicy({ overrides: { 'plugin>constructor@^1.0.0': 'link:./c' } }),
    new Map<string, Record<string, string>>([
      [pristineSpecMapKey('plugin@1.0.0', 'dependencies'), {}],
      [pristineSpecMapKey('plugin@1.0.0', 'peerDependencies'), { constructor: '^1.0.0' }],
    ])
  )
  await expect(verifyLockfileResolutions(lockfile, [], { localPolicy: policy })).resolves.toBeUndefined()
})

posixOnly('the first verification pass anchors file URL keys on the lockfile directory', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { helper: 'file:///work/helper' },
        specifiers: { helper: 'file:../helper' },
      },
    },
    packages: {
      ['helper@file:///work/helper' as DepPath]: {
        resolution: { type: 'directory', directory: '../helper' },
      },
    },
  }
  await expect(verifyLockfileResolutions(lockfile, [], { lockfilePath: '/work/app/pnpm-lock.yaml' })).resolves.toBeUndefined()
})

posixOnly('a deployed directory named like a tarball matches its target', () => {
  const lockfile = (directory: string): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { helper: 'file:///work/helper.tgz' },
        specifiers: { helper: 'file:../helper.tgz' },
      },
    },
    packages: {
      ['helper@file:///work/helper.tgz' as DepPath]: {
        resolution: { type: 'directory', directory },
      },
    },
  })
  const deployPolicy = { ...buildPolicy({ lockfileDir: '/work/app' }), allowDeployFileUrlKeys: true }
  expect(() => verifyLockfileResolutionShapes(lockfile('../helper.tgz'), deployPolicy)).not.toThrow()
  expect(() => verifyLockfileResolutionShapes(lockfile('../other'), deployPolicy)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  }))
})

test('explicit and hashed peer suffixes both bind a local peer', async () => {
  const lockfile = (suffix: string): LockfileObject => ({
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { plugin: `1.0.0${suffix}` },
        specifiers: { plugin: '1.0.0' },
      },
    },
    packages: {
      [`plugin@1.0.0${suffix}` as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: '1.0.0' },
        peerDependencies: { helper: 'file:../unused' },
      },
      ['helper@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-helper' },
      },
    },
  })
  for (const suffix of ['(helper@1.0.0)', '(0123456789abcdef0123456789abcdef)', '(patch_hash=abc)(0123456789abcdef0123456789abcdef)']) {
    // eslint-disable-next-line no-await-in-loop -- sequential assertion per suffix form
    await expect(verifyLockfileResolutions(lockfile(suffix), [], { localPolicy: buildPolicy({}) })).rejects.toMatchObject({
      code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    })
  }
  await expect(verifyLockfileResolutions(lockfile('(patch_hash=abc)'), [], { localPolicy: buildPolicy({}) })).resolves.toBeUndefined()
})
