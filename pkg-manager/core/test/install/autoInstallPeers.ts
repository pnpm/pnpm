import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { addDependenciesToPackage, install, mutateModules, mutateModulesInSingleProject, PackageManifest } from '@pnpm/core'
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

// Covers https://github.com/pnpm/pnpm/issues/5373
test('prefer the peer dependency version already used in the root', async () => {
  await addDistTag({ package: '@pnpm/y', version: '2.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await install({
    peerDependencies: {
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '^1.0.0',
    },
  }, await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages).sort()).toStrictEqual([
    '/@pnpm/y/1.0.0',
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
  await mutateModulesInSingleProject({
    dependencyNames: ['is-odd'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd(),
  }, await testDefaults({ autoInstallPeers: true }))
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
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'project-1',
          devDependencies: {
            'is-positive': '1.0.0',
          },
        },
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
        rootDir: path.resolve('project-2'),
      },
    ],
    autoInstallPeers: true,
  }))

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
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
  ], await testDefaults({
    autoInstallPeers: true,
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'root-project',
          devDependencies: {
            '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
          },
        },
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
        rootDir: path.resolve('project'),
      },
    ],
  }))
})

test('auto install peer deps in a workspace. test #2', async () => {
  prepareEmpty()
  await mutateModules([
    {
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
  ], await testDefaults({
    autoInstallPeers: true,
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'root-project',
          devDependencies: {
            '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
            '@pnpm.e2e/peer-c': '1.0.0',
          },
        },
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
        rootDir: path.resolve('project'),
      },
    ],
  }))
})

// This test may be removed if autoInstallPeers will become true by default
test('installation on a package with many complex circular dependencies does not fail when auto install peers is on', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['webpack@4.46.0'], await testDefaults({ autoInstallPeers: true }))
})

// This test may be removed if autoInstallPeers will become true by default
test('installation on a workspace with many complex circular dependencies does not fail when auto install peers is on', async () => {
  prepareEmpty()
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project2'),
    },
  ], await testDefaults({
    autoInstallPeers: true,
    ignoreScripts: true,
    lockfileOnly: true,
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'project1',
          dependencies: {
            '@angular/common': '14.2.4',
            '@angular/core': '14.2.4',
            '@angular/forms': '14.2.4',
            '@angular/platform-browser': '14.2.4',
            '@angular/platform-browser-dynamic': '14.2.4',
            '@angular/router': '14.2.4',
            '@capacitor/app': '4.0.1',
            '@capacitor/core': '4.3.0',
            '@capacitor/haptics': '4.0.1',
            '@capacitor/keyboard': '4.0.1',
            '@capacitor/status-bar': '4.0.1',
            '@ionic/angular': '6.2.9',
            '@ionic/core': '6.2.9',
            ionicons: '6.0.3',
            'ng-particles': '3.3.3',
            rxjs: '7.5.7',
            tslib: '2.4.0',
            tsparticles: '2.3.4',
            'tsparticles-engine': '2.3.3',
            'tsparticles-interaction-external-attract': '2.3.3',
            'tsparticles-interaction-external-bounce': '2.3.3',
            'tsparticles-interaction-external-bubble': '2.3.3',
            'tsparticles-interaction-external-connect': '2.3.3',
            'tsparticles-interaction-external-grab': '2.3.3',
            'tsparticles-interaction-external-pause': '2.3.3',
            'tsparticles-interaction-external-push': '2.3.3',
            'tsparticles-interaction-external-remove': '2.3.3',
            'tsparticles-interaction-external-repulse': '2.3.4',
            'tsparticles-interaction-external-slow': '2.3.3',
            'tsparticles-interaction-external-trail': '2.3.3',
            'tsparticles-interaction-particles-attract': '2.3.3',
            'tsparticles-interaction-particles-collisions': '2.3.3',
            'tsparticles-interaction-particles-links': '2.3.3',
            'tsparticles-move-base': '2.3.3',
            'tsparticles-move-parallax': '2.3.3',
            'tsparticles-particles.js': '2.3.3',
            'tsparticles-plugin-absorbers': '2.3.4',
            'tsparticles-plugin-emitters': '2.3.4',
            'tsparticles-plugin-polygon-mask': '2.3.3',
            'tsparticles-shape-circle': '2.3.3',
            'tsparticles-shape-image': '2.3.3',
            'tsparticles-shape-line': '2.3.3',
            'tsparticles-shape-polygon': '2.3.3',
            'tsparticles-shape-square': '2.3.3',
            'tsparticles-shape-star': '2.3.3',
            'tsparticles-shape-text': '2.3.3',
            'tsparticles-slim': '2.3.4',
            'tsparticles-updater-angle': '2.3.3',
            'tsparticles-updater-color': '2.3.3',
            'tsparticles-updater-destroy': '2.3.3',
            'tsparticles-updater-life': '2.3.3',
            'tsparticles-updater-opacity': '2.3.3',
            'tsparticles-updater-out-modes': '2.3.3',
            'tsparticles-updater-roll': '2.3.3',
            'tsparticles-updater-size': '2.3.3',
            'tsparticles-updater-stroke-color': '2.3.3',
            'tsparticles-updater-tilt': '2.3.3',
            'tsparticles-updater-twinkle': '2.3.3',
            'tsparticles-updater-wobble': '2.3.3',
            'zone.js': '0.11.8',
          },
          devDependencies: {
            '@angular-devkit/build-angular': '14.2.4',
            '@angular-eslint/builder': '14.1.2',
            '@angular-eslint/eslint-plugin': '14.1.2',
            '@angular-eslint/eslint-plugin-template': '14.1.2',
            '@angular-eslint/template-parser': '14.1.2',
            '@angular/cli': '14.2.4',
            '@angular/compiler': '14.2.4',
            '@angular/compiler-cli': '14.2.4',
            '@angular/language-service': '14.2.4',
            '@capacitor/cli': '4.3.0',
            '@ionic/angular-toolkit': '7.0.0',
            '@types/jasmine': '4.3.0',
            '@types/jasminewd2': '2.0.10',
            '@types/node': '18.7.23',
            '@typescript-eslint/eslint-plugin': '5.38.1',
            '@typescript-eslint/parser': '5.38.1',
            eslint: '8.24.0',
            'eslint-plugin-import': '2.26.0',
            'eslint-plugin-jsdoc': '39.3.6',
            'eslint-plugin-prefer-arrow': '1.2.3',
            'jasmine-core': '4.4.0',
            'jasmine-spec-reporter': '7.0.0',
            karma: '6.4.1',
            'karma-chrome-launcher': '3.1.1',
            'karma-coverage': '2.2.0',
            'karma-coverage-istanbul-reporter': '3.0.3',
            'karma-jasmine': '5.1.0',
            'karma-jasmine-html-reporter': '2.0.0',
            protractor: '7.0.0',
            'ts-node': '10.9.1',
            typescript: '4.8.4',
          },
        },
        rootDir: path.resolve('project1'),
      },
      {
        buildIndex: 0,
        manifest: {
          name: 'project2',
          devDependencies: {
            '@typescript-eslint/eslint-plugin': '5.38.1',
            '@typescript-eslint/parser': '5.38.1',
            copyfiles: '2.4.1',
            enzyme: '3.11.0',
            'enzyme-adapter-preact-pure': '4.0.1',
            eslint: '8.24.0',
            'eslint-config-preact': '1.3.0',
            'eslint-config-prettier': '8.5.0',
            'eslint-plugin-react-hooks': '4.6.0',
            'identity-obj-proxy': '3.0.0',
            'preact-cli': '3.4.1',
            prettier: '2.7.1',
            'sirv-cli': '2.0.2',
          },
          dependencies: {
            preact: '10.11.0',
            'preact-particles': '2.3.3',
            'preact-render-to-string': '5.2.4',
            'preact-router': '4.1.0',
            tsparticles: '2.3.4',
            'tsparticles-engine': '2.3.3',
          },
        },
        rootDir: path.resolve('project2'),
      },
    ],
  }))
})

test('do not override the direct dependency with an auto installed peer dependency', async () => {
  const includedDeps = new Set([
    '@angular-devkit/build-angular',
    '@angular/platform-browser-dynamic',
    'inquirer',
    'rxjs',
    '@angular/common',
    'rxjs',
  ])
  const project = prepareEmpty()
  await install({
    dependencies: {
      rxjs: '6.6.7',
    },
    devDependencies: {
      'jest-preset-angular': '12.0.1',
    },
  }, await testDefaults({
    autoInstallPeers: true,
    hooks: {
      // This hook may be removed and the test will still be valid.
      // The only reason the hook was added to remove the packages that aren't needed for the tests and make the test faster.
      readPackage: [
        (pkg: PackageManifest) => {
          for (const depType of ['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta']) {
            if (pkg[depType]) {
              for (const depName of Object.keys(pkg[depType])) {
                if (!includedDeps.has(depName)) {
                  delete pkg[depType][depName]
                }
              }
            }
          }
          if (pkg.name === '@angular-devkit/build-angular' && pkg.dependencies) {
            delete pkg.dependencies.rxjs
          }
          return pkg
        },
      ],
    },
  }))
  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies.rxjs).toStrictEqual('6.6.7')
})
