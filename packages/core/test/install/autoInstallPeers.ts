import path from 'path'
import assertProject from '@pnpm/assert-project'
import { addDependenciesToPackage, install, mutateModules } from '@pnpm/core'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'
import { createPeersFolderSuffix } from 'dependency-path'
import { testDefaults } from '../utils'

test('auto install non-optional peer dependencies', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/abc-optional-peers@1.0.0'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/abc-optional-peers/1.0.0_@pnpm.e2e+peer-a@1.0.0',
    '/@pnpm.e2e/peer-a/1.0.0',
  ])
  await project.hasNot('@pnpm.e2e/peer-a')
})

test('auto install the common peer dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.1', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/wants-peer-c-1', '@pnpm.e2e/wants-peer-c-1.0.0'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/peer-c/1.0.0',
    '/@pnpm.e2e/wants-peer-c-1.0.0/1.0.0_@pnpm.e2e+peer-c@1.0.0',
    '/@pnpm.e2e/wants-peer-c-1/1.0.0_@pnpm.e2e+peer-c@1.0.0',
  ])
})

test('do not auto install when there is no common peer dependency range intersection', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/wants-peer-c-1', '@pnpm.e2e/wants-peer-c-2'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/wants-peer-c-1/1.0.0',
    '/@pnpm.e2e/wants-peer-c-2/1.0.0',
  ])
})

test('don\'t fail on linked package, when peers are auto installed', async () => {
  const pkgManifest = {
    dependencies: {
      linked: 'link:../linked',
    },
  }
  preparePackages([
    {
      location: 'linked',
      package: {
        name: 'linked',
        peerDependencies: {
          'peer-c': '1.0.0',
        },
      },
    },
    {
      location: 'pkg',
      package: pkgManifest,
    },
  ])
  process.chdir('pkg')
  const updatedManifest = await addDependenciesToPackage(pkgManifest, ['@pnpm.e2e/peer-b'], await testDefaults({ autoInstallPeers: true }))
  expect(Object.keys(updatedManifest.dependencies ?? {})).toStrictEqual(['linked', '@pnpm.e2e/peer-b'])
})

test('hoist a peer dependency in order to reuse it by other dependencies, when it satisfies them', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm/xyz-parent-parent-parent-parent', '@pnpm/xyz-parent-parent-with-xyz'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  const suffix = createPeersFolderSuffix([{ name: '@pnpm/x', version: '1.0.0' }, { name: '@pnpm/y', version: '1.0.0' }, { name: '@pnpm/z', version: '1.0.0' }])
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm/x/1.0.0',
    `/@pnpm/xyz-parent-parent-parent-parent/1.0.0${suffix}`,
    `/@pnpm/xyz-parent-parent-parent/1.0.0${suffix}`,
    '/@pnpm/xyz-parent-parent-with-xyz/1.0.0',
    `/@pnpm/xyz-parent-parent/1.0.0${suffix}`,
    `/@pnpm/xyz-parent/1.0.0${suffix}`,
    `/@pnpm/xyz/1.0.0${suffix}`,
    '/@pnpm/y/1.0.0',
    '/@pnpm/z/1.0.0',
  ])
})

test('don\'t hoist a peer dependency when there is a root dependency by that name', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm/xyz-parent-parent-parent-parent',
    '@pnpm/xyz-parent-parent-with-xyz',
    '@pnpm/x@npm:@pnpm.e2e/peer-a@1.0.0',
    `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm/y/-/y-2.0.0.tgz`,
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  const suffix1 = createPeersFolderSuffix([{ name: '@pnpm/y', version: '2.0.0' }, { name: '@pnpm/z', version: '1.0.0' }, { name: '@pnpm.e2e/peer-a', version: '1.0.0' }])
  const suffix2 = createPeersFolderSuffix([{ name: '@pnpm/x', version: '1.0.0' }, { name: '@pnpm/y', version: '1.0.0' }, { name: '@pnpm/z', version: '1.0.0' }])
  expect(Object.keys(lockfile.packages).sort()).toStrictEqual([
    '/@pnpm.e2e/peer-a/1.0.0',
    '/@pnpm/x/1.0.0',
    `/@pnpm/xyz-parent-parent-parent-parent/1.0.0${suffix1}`,
    `/@pnpm/xyz-parent-parent-parent/1.0.0${suffix1}`,
    '/@pnpm/xyz-parent-parent-with-xyz/1.0.0',
    `/@pnpm/xyz-parent-parent/1.0.0${suffix1}`,
    `/@pnpm/xyz-parent/1.0.0${suffix1}`,
    `/@pnpm/xyz-parent/1.0.0${suffix2}`,
    `/@pnpm/xyz/1.0.0${suffix1}`,
    `/@pnpm/xyz/1.0.0${suffix2}`,
    '/@pnpm/y/1.0.0',
    '/@pnpm/y/2.0.0',
    '/@pnpm/z/1.0.0',
  ].sort())
})

test('don\'t auto-install a peer dependency, when that dependency is in the root', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm/xyz-parent-parent-parent-parent',
    '@pnpm/x@npm:@pnpm.e2e/peer-a@1.0.0',
    `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm/y/-/y-2.0.0.tgz`,
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  const suffix = createPeersFolderSuffix([{ name: '@pnpm/y', version: '2.0.0' }, { name: '@pnpm/z', version: '1.0.0' }, { name: '@pnpm.e2e/peer-a', version: '1.0.0' }])
  expect(Object.keys(lockfile.packages).sort()).toStrictEqual([
    `/@pnpm/xyz-parent-parent-parent-parent/1.0.0${suffix}`,
    `/@pnpm/xyz-parent-parent-parent/1.0.0${suffix}`,
    `/@pnpm/xyz-parent-parent/1.0.0${suffix}`,
    `/@pnpm/xyz-parent/1.0.0${suffix}`,
    `/@pnpm/xyz/1.0.0${suffix}`,
    '/@pnpm/y/2.0.0',
    '/@pnpm/z/1.0.0',
    '/@pnpm.e2e/peer-a/1.0.0',
  ].sort())
})

test('don\'t install the same missing peer dependency twice', async () => {
  await addDistTag({ package: '@pnpm/y', version: '2.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm.e2e/has-has-y-peer-peer',
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages).sort()).toStrictEqual([
    '/@pnpm/y/1.0.0',
    `/@pnpm.e2e/has-has-y-peer-peer/1.0.0${createPeersFolderSuffix([{ name: '@pnpm/y', version: '1.0.0' }, { name: '@pnpm.e2e/has-y-peer', version: '1.0.0' }])}`,
    '/@pnpm.e2e/has-y-peer/1.0.0_@pnpm+y@1.0.0',
  ].sort())
})

test('automatically install root peer dependencies', async () => {
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      'is-negative': '^1.0.0',
    },
    peerDependencies: {
      'is-positive': '^1.0.0',
    },
  }, await testDefaults({ autoInstallPeers: true }))

  await project.has('is-positive')
  await project.has('is-negative')

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.specifiers).toStrictEqual({
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    })
    expect(lockfile.dependencies).toStrictEqual({
      'is-positive': '1.0.0',
      'is-negative': '1.0.1',
    })
  }

  // Automatically install the peer dependency when the lockfile is up to date
  await rimraf('node_modules')

  await install(manifest, await testDefaults({ autoInstallPeers: true, frozenLockfile: true }))

  await project.has('is-positive')
  await project.has('is-negative')

  // The auto installed peer is not removed when a new dependency is added
  manifest = await addDependenciesToPackage(manifest, ['is-odd@1.0.0'], await testDefaults({ autoInstallPeers: true }))
  await project.has('is-odd')
  await project.has('is-positive')
  await project.has('is-negative')

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.specifiers).toStrictEqual({
      'is-odd': '1.0.0',
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    })
    expect(lockfile.dependencies).toStrictEqual({
      'is-odd': '1.0.0',
      'is-positive': '1.0.0',
      'is-negative': '1.0.1',
    })
  }

  // The auto installed peer is not removed when a dependency is removed
  await mutateModules([
    {
      dependencyNames: ['is-odd'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ autoInstallPeers: true }))
  await project.hasNot('is-odd')
  await project.has('is-positive')
  await project.has('is-negative')

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.specifiers).toStrictEqual({
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    })
    expect(lockfile.dependencies).toStrictEqual({
      'is-positive': '1.0.0',
      'is-negative': '1.0.1',
    })
  }
})

test('automatically install peer dependency when it is a dev dependency in another workspace project', async () => {
  prepareEmpty()

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        devDependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        peerDependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({ autoInstallPeers: true }))

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.importers['project-1'].devDependencies).toStrictEqual({
    'is-positive': '1.0.0',
  })
  expect(lockfile.importers['project-2'].dependencies).toStrictEqual({
    'is-positive': '1.0.0',
  })
})

// Covers https://github.com/pnpm/pnpm/issues/4820
test('auto install peer deps in a workspace. test #1', async () => {
  prepareEmpty()
  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        name: 'root-project',
        devDependencies: {
          '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        peerDependencies: {
          '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
  ], await testDefaults({ autoInstallPeers: true }))
})

test('auto install peer deps in a workspace. test #2', async () => {
  prepareEmpty()
  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        name: 'root-project',
        devDependencies: {
          '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
          '@pnpm.e2e/peer-c': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        peerDependencies: {
          '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
  ], await testDefaults({ autoInstallPeers: true }))
})
