import { promises as fs } from 'fs'
import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { RootLog } from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import { Lockfile, TarballResolution } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { addDistTag, getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { ProjectManifest } from '@pnpm/types'
import readYamlFile from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import exists from 'path-exists'
import sinon from 'sinon'
import writeYamlFile from 'write-yaml-file'
import { testDefaults } from './utils'

const LOCKFILE_WARN_LOG = {
  level: 'warn',
  message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
  name: 'pnpm',
}

test('lockfile has correct format', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep',
      '@rstacruz/tap-spec@4.1.1',
      'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c',
    ], await testDefaults({ fastUnpack: false, save: true }))

  const modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds.length).toBe(0)

  const lockfile = await project.readLockfile()
  const id = '/@pnpm.e2e/pkg-with-1-dep/100.0.0'

  expect(lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)

  expect(lockfile.specifiers).toBeTruthy()
  expect(lockfile.dependencies).toBeTruthy()
  expect(lockfile.dependencies['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.dependencies).toHaveProperty(['@rstacruz/tap-spec'])
  expect(lockfile.dependencies['is-negative']).toContain('/') // has not shortened tarball from the non-standard registry

  expect(lockfile.packages).toBeTruthy() // has packages field
  expect(lockfile.packages).toHaveProperty([id])
  expect(lockfile.packages[id].dependencies).toBeTruthy()
  expect(lockfile.packages[id].dependencies).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep'])
  expect(lockfile.packages[id].resolution).toBeTruthy()
  expect((lockfile.packages[id].resolution as {integrity: string}).integrity).toBeTruthy()
  expect((lockfile.packages[id].resolution as TarballResolution).tarball).toBeFalsy()

  const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
  expect(lockfile.packages).toHaveProperty([absDepPath])
  expect(lockfile.packages[absDepPath].name).toBeTruthy() // github-hosted package has name specified
})

test('lockfile has dev deps even when installing for prod only', async () => {
  const project = prepareEmpty()

  await install({
    devDependencies: {
      'is-negative': '2.1.0',
    },
  }, await testDefaults({ production: true }))

  const lockfile = await project.readLockfile()
  const id = '/is-negative/2.1.0'

  expect(lockfile.devDependencies).toBeTruthy()

  expect(lockfile.devDependencies['is-negative']).toBe('2.1.0')

  expect(lockfile.packages[id]).toBeTruthy()
})

test('lockfile with scoped package', async () => {
  prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
        },
      },
    },
    specifiers: {
      '@types/semver': '^5.3.31',
    },
  }, { lineWidth: 1000 })

  await install({
    dependencies: {
      '@types/semver': '^5.3.31',
    },
  }, await testDefaults({ frozenLockfile: true }))
})

test("lockfile doesn't lock subdependencies that don't satisfy the new specs", async () => {
  const project = prepareEmpty()

  // dependends on react-onclickoutside@5.9.0
  const manifest = await addDependenciesToPackage({}, ['react-datetime@2.8.8'], await testDefaults({ fastUnpack: false, save: true, strictPeerDependencies: false }))

  // dependends on react-onclickoutside@0.3.4
  await addDependenciesToPackage(manifest, ['react-datetime@1.3.0'], await testDefaults({ save: true, strictPeerDependencies: false }))

  expect(
    project.requireModule('.pnpm/react-datetime@1.3.0/node_modules/react-onclickoutside/package.json').version
  ).toBe('0.3.4') // react-datetime@1.3.0 has react-onclickoutside@0.3.4 in its node_modules

  const lockfile = await project.readLockfile()

  expect(Object.keys(lockfile.dependencies).length).toBe(1) // resolutions not duplicated
})

test('lockfile not created when no deps in package.json', async () => {
  const project = prepareEmpty()

  await install({}, await testDefaults())

  expect(await project.readLockfile()).toBeFalsy()
  expect(await exists('node_modules')).toBeFalsy()
})

test('lockfile removed when no deps in package.json', async () => {
  const project = prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      'is-negative': '2.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-negative/-/is-negative-2.1.0.tgz`,
        },
      },
    },
    specifiers: {
      'is-negative': '2.1.0',
    },
  }, { lineWidth: 1000 })

  await install({}, await testDefaults())

  expect(await project.readLockfile()).toBeFalsy()
})

test('lockfile is fixed when it does not match package.json', async () => {
  const project = prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
        },
      },
      '/is-negative/2.1.0': {
        resolution: {
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-negative/-/is-negative-2.1.0.tgz`,
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  }, { lineWidth: 1000 })

  const reporter = sinon.spy()
  await install({
    devDependencies: {
      'is-negative': '^2.1.0',
    },
    optionalDependencies: {
      'is-positive': '^3.1.0',
    },
  }, await testDefaults({ reporter }))

  const progress = sinon.match({
    name: 'pnpm:progress',
    status: 'resolving',
  })
  expect(reporter.withArgs(progress).callCount).toBe(0)

  const lockfile = await project.readLockfile()

  expect(lockfile.devDependencies['is-negative']).toBe('2.1.0')
  expect(lockfile.optionalDependencies['is-positive']).toBe('3.1.0')
  expect(lockfile.dependencies).toBeFalsy()
  expect(lockfile.packages).not.toHaveProperty(['/@types/semver/5.3.31'])
})

test(`doing named installation when ${WANTED_LOCKFILE} exists already`, async () => {
  const project = prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
        },
      },
      '/is-negative/2.1.0': {
        resolution: {
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-negative/-/is-negative-2.1.0.tgz`,
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  }, { lineWidth: 1000 })

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  }, ['is-positive'], await testDefaults({ reporter }))
  await install(manifest, await testDefaults({ reporter }))

  expect(reporter.calledWithMatch(LOCKFILE_WARN_LOG)).toBeFalsy()

  await project.has('is-negative')
})

test(`respects ${WANTED_LOCKFILE} for top dependencies`, async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()
  // const fooProgress = sinon.match({
  //   name: 'pnpm:progress',
  //   status: 'resolving',
  //   manifest: {
  //     name: 'foo',
  //   },
  // })

  const pkgs = ['@pnpm.e2e/foo', '@pnpm.e2e/bar', '@pnpm.e2e/qar']
  await Promise.all(pkgs.map(async (pkgName) => addDistTag({ package: pkgName, version: '100.0.0', distTag: 'latest' })))

  let manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foo'], await testDefaults({ save: true, reporter }))
  // t.equal(reporter.withArgs(fooProgress).callCount, 1, 'reported foo once')
  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/bar'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/qar'], await testDefaults({ addDependenciesToPackage: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/foobar'], await testDefaults({ save: true }))

  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/foo'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/bar'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/qar'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules/.pnpm/@pnpm.e2e+foobar@100.0.0/node_modules/@pnpm.e2e/foo'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules/.pnpm/@pnpm.e2e+foobar@100.0.0/node_modules/@pnpm.e2e/bar'))).version).toBe('100.0.0')

  await Promise.all(pkgs.map(async (pkgName) => addDistTag({ package: pkgName, version: '100.1.0', distTag: 'latest' })))

  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  reporter.resetHistory()

  // shouldn't care about what the registry in npmrc is
  // the one in lockfile should be used
  await install(manifest, await testDefaults({
    rawConfig: {
      registry: 'https://registry.npmjs.org',
    },
    registry: 'https://registry.npmjs.org',
    reporter,
  }))

  // t.equal(reporter.withArgs(fooProgress).callCount, 0, 'not reported foo')

  await project.storeHasNot('@pnpm.e2e/foo', '100.1.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/foo'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/bar'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules', '@pnpm.e2e/qar'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules/.pnpm/@pnpm.e2e+foobar@100.0.0/node_modules/@pnpm.e2e/foo'))).version).toBe('100.0.0')
  expect((await readPackageJsonFromDir(path.resolve('node_modules/.pnpm/@pnpm.e2e+foobar@100.0.0/node_modules/@pnpm.e2e/bar'))).version).toBe('100.0.0')
})

test(`subdeps are updated on repeat install if outer ${WANTED_LOCKFILE} does not match the inner one`, async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults())

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])

  delete lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0']

  lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'] = {
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
    },
  }

  lockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].dependencies!['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'

  await writeYamlFile(WANTED_LOCKFILE, lockfile, { lineWidth: 1000 })

  await install(manifest, await testDefaults())

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
})

test("recreates lockfile if it doesn't match the dependencies in package.json", async () => {
  const project = prepareEmpty()

  let manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['is-positive@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['map-obj@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'optionalDependencies' }))

  const lockfile1 = await project.readLockfile()
  expect(lockfile1.dependencies['is-negative']).toBe('1.0.0')
  expect(lockfile1.specifiers['is-negative']).toBe('1.0.0')

  manifest.dependencies!['is-negative'] = '^2.1.0'
  manifest.devDependencies!['is-positive'] = '^2.0.0'
  manifest.optionalDependencies!['map-obj'] = '1.0.1'

  await install(manifest, await testDefaults())

  const lockfile = await project.readLockfile()

  expect(lockfile.dependencies['is-negative']).toBe('2.1.0')
  expect(lockfile.specifiers['is-negative']).toBe('^2.1.0')

  expect(lockfile.devDependencies['is-positive']).toBe('2.0.0')
  expect(lockfile.specifiers['is-positive']).toBe('^2.0.0')

  expect(lockfile.optionalDependencies['map-obj']).toBe('1.0.1')
  expect(lockfile.specifiers['map-obj']).toBe('1.0.1')
})

test('repeat install with lockfile should not mutate lockfile when dependency has version specified with v prefix', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['highmaps-release@5.0.11'], await testDefaults())

  const lockfile1 = await project.readLockfile()

  expect(lockfile1.dependencies['highmaps-release']).toBe('5.0.11')

  await rimraf('node_modules')

  await install(manifest, await testDefaults())

  const lockfile2 = await project.readLockfile()

  expect(lockfile1).toStrictEqual(lockfile2) // lockfile hasn't been changed
})

test('package is not marked dev if it is also a subdep of a regular dependency', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults())

  console.log('installed @pnpm.e2e/pkg-with-1-dep')

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  console.log('installed optional dependency which is also a dependency of @pnpm.e2e/pkg-with-1-dep')

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'].dev).toBeFalsy()
})

test('package is not marked optional if it is also a subdep of a regular dependency', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults())
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'].optional).toBeFalsy()
})

test('scoped module from different registry', async () => {
  const project = prepareEmpty()

  const opts = await testDefaults()
  opts.registries!.default = 'https://registry.npmjs.org/'
  opts.registries!['@zkochan'] = `http://localhost:${REGISTRY_MOCK_PORT}`
  opts.registries!['@foo'] = `http://localhost:${REGISTRY_MOCK_PORT}`
  await addDependenciesToPackage({}, ['@zkochan/foo', '@foo/has-dep-from-same-scope', 'is-positive'], opts)

  await project.has('@zkochan/foo')

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    dependencies: {
      '@foo/has-dep-from-same-scope': '1.0.0',
      '@zkochan/foo': '1.0.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@foo/has-dep-from-same-scope/1.0.0': {
        dependencies: {
          '@foo/no-deps': '1.0.0',
          'is-negative': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('@foo/has-dep-from-same-scope', '1.0.0'),
        },
      },
      '/@foo/no-deps/1.0.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('@foo/no-deps', '1.0.0'),
        },
      },
      '/@zkochan/foo/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha512-IFvrYpq7E6BqKex7A7czIFnFncPiUVdhSzGhAOWpp8RlkXns4y/9ZdynxaA/e0VkihRxQkihE2pTyvxjfe/wBg==',
        },
      },
      '/is-negative/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    specifiers: {
      '@foo/has-dep-from-same-scope': '^1.0.0',
      '@zkochan/foo': '^1.0.0',
      'is-positive': '^3.1.0',
    },
  })
})

test('repeat install with no inner lockfile should not rewrite packages in node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], await testDefaults())

  await rimraf('node_modules/.pnpm/lock.yaml')

  await install(manifest, await testDefaults())

  await project.has('is-negative')
})

test('packages are placed in devDependencies even if they are present as non-dev as well', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const reporter = sinon.spy()
  await install({
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.1.0',
      '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
    },
  }, await testDefaults({ reporter }))

  const lockfile = await project.readLockfile()

  expect(lockfile.devDependencies).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep'])
  expect(lockfile.devDependencies).toHaveProperty(['@pnpm.e2e/pkg-with-1-dep'])

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      version: '100.1.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: '@pnpm.e2e/pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()
})

// This testcase verifies that pnpm is not failing when trying to preserve dependencies.
// Only when a dependency is a range dependency, should pnpm try to compare versions of deps with semver.satisfies().
test('updating package that has a github-hosted dependency', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-github-dep@1'], await testDefaults())
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/has-github-dep@latest'], await testDefaults())
})

test('updating package that has deps with peers', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent-with-c@0'], await testDefaults())
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc-grand-parent-with-c@1'], await testDefaults())
})

test('pendingBuilds gets updated if install removes packages', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      '@pnpm.e2e/with-postinstall-b': '*',
    },
  }, await testDefaults({ fastUnpack: false, ignoreScripts: true }))
  const modules1 = await project.readModulesManifest()

  await install({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
    },
  }, await testDefaults({ fastUnpack: false, ignoreScripts: true }))
  const modules2 = await project.readModulesManifest()

  expect(modules1).toBeTruthy()
  expect(modules2).toBeTruthy()
  expect(modules1!.pendingBuilds.length > modules2!.pendingBuilds.length).toBeTruthy()
})

test('dev properties are correctly updated on named install', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    ['inflight@1.0.6'],
    await testDefaults({ targetDependenciesField: 'devDependencies' })
  )
  await addDependenciesToPackage(manifest, ['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const lockfile = await project.readLockfile()
  expect(
    Object.values(lockfile.packages).filter((dep) => typeof dep.dev !== 'undefined')
  ).toStrictEqual([])
})

test('optional properties are correctly updated on named install', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['inflight@1.0.6'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await addDependenciesToPackage(manifest, ['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const lockfile = await project.readLockfile()
  expect(Object.values(lockfile.packages).filter((dep) => typeof dep.optional !== 'undefined')).toStrictEqual([])
})

test('dev property is correctly set for package that is duplicated to both the dependencies and devDependencies group', async () => {
  const project = prepareEmpty()

  // TODO: use a smaller package for testing
  await addDependenciesToPackage({}, ['overlap@2.2.8'], await testDefaults())

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/couleurs/5.0.0'].dev === false).toBeTruthy()
})

test('no lockfile', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ useLockfile: false, reporter }))

  expect(reporter.calledWithMatch(LOCKFILE_WARN_LOG)).toBeFalsy()

  await project.has('is-positive')

  expect(await project.readLockfile()).toBeFalsy()
})

test('lockfile is ignored when lockfile = false', async () => {
  const project = prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      'is-negative': '2.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp10=', // Invalid integrity
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-negative/-/is-negative-2.1.0.tgz`,
        },
      },
    },
    specifiers: {
      'is-negative': '2.1.0',
    },
  }, { lineWidth: 1000 })

  const reporter = sinon.spy()

  await install({
    dependencies: {
      'is-negative': '2.1.0',
    },
  }, await testDefaults({ useLockfile: false, reporter }))

  expect(reporter.calledWithMatch(LOCKFILE_WARN_LOG)).toBeTruthy()

  await project.has('is-negative')

  expect(await project.readLockfile()).toBeTruthy()
})

test(`don't update ${WANTED_LOCKFILE} during uninstall when useLockfile: false`, async () => {
  const project = prepareEmpty()

  let manifest!: ProjectManifest
  {
    const reporter = sinon.spy()

    manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ reporter }))

    expect(reporter.calledWithMatch(LOCKFILE_WARN_LOG)).toBeFalsy()
  }

  {
    const reporter = sinon.spy()

    await mutateModules([
      {
        dependencyNames: ['is-positive'],
        manifest,
        mutation: 'uninstallSome',
        rootDir: process.cwd(),
      },
    ], await testDefaults({ useLockfile: false, reporter }))

    expect(reporter.calledWithMatch(LOCKFILE_WARN_LOG)).toBeTruthy()
  }

  await project.hasNot('is-positive')

  expect(await project.readLockfile()).toBeTruthy()
})

test('fail when installing with useLockfile: false and lockfileOnly: true', async () => {
  prepareEmpty()

  try {
    await install({}, await testDefaults({ useLockfile: false, lockfileOnly: true }))
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toBe(`Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
  }
})

test("don't remove packages during named install when useLockfile: false", async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ useLockfile: false }))
  await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ useLockfile: false }))

  await project.has('is-positive')
  await project.has('is-negative')
})

test('save tarball URL when it is non-standard', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['esprima-fb@3001.1.0-dev-harmony-fb'], await testDefaults({ fastUnpack: false }))

  const lockfile = await project.readLockfile()

  expect((lockfile.packages['/esprima-fb/3001.1.0-dev-harmony-fb'].resolution as TarballResolution).tarball).toBe('esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz')
})

test('packages installed via tarball URL from the default registry are normalized', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, [
    `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz`,
    'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  ], await testDefaults())

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    dependencies: {
      'is-positive': '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      '@pnpm.e2e/pkg-with-tarball-dep-from-registry': '1.0.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0'),
        },
      },
      '/@pnpm.e2e/pkg-with-tarball-dep-from-registry/1.0.0': {
        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/pkg-with-tarball-dep-from-registry', '1.0.0'),
        },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        dev: false,
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        resolution: {
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        version: '1.0.0',
      },
    },
    specifiers: {
      'is-positive': 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      '@pnpm.e2e/pkg-with-tarball-dep-from-registry': `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz`,
    },
  })
})

test('lockfile file has correct format when lockfile directory does not equal the prefix directory', async () => {
  prepareEmpty()

  const storeDir = path.resolve('..', '.store')

  const manifest = await addDependenciesToPackage(
    {},
    [
      '@pnpm.e2e/pkg-with-1-dep',
      '@zkochan/foo@1.0.0',
      'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c',
    ],
    await testDefaults({ save: true, lockfileDir: path.resolve('..'), storeDir })
  )

  expect(!await exists('node_modules/.modules.yaml')).toBeTruthy()

  process.chdir('..')

  const modules = await readYamlFile<object>(path.resolve('node_modules', '.modules.yaml'))
  expect(modules).toBeTruthy()
  expect(modules['pendingBuilds'].length).toBe(0)

  {
    const lockfile: Lockfile = await readYamlFile(WANTED_LOCKFILE)
    const id = '/@pnpm.e2e/pkg-with-1-dep/100.0.0'

    expect(lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)

    expect(lockfile.importers).toBeTruthy()
    expect(lockfile.importers.project).toBeTruthy()
    expect(lockfile.importers.project.specifiers).toBeTruthy()
    expect(lockfile.importers.project.dependencies).toBeTruthy()
    expect(lockfile.importers.project.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')
    expect(lockfile.importers.project.dependencies!['@zkochan/foo']).toBeTruthy()
    expect(lockfile.importers.project.dependencies!['is-negative']).toContain('/')

    expect(lockfile.packages![id].dependencies).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep'])
    expect(lockfile.packages![id].resolution).toHaveProperty(['integrity'])
    expect(lockfile.packages![id].resolution).not.toHaveProperty(['tarball'])

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    expect(lockfile.packages).toHaveProperty([absDepPath])
    expect(lockfile.packages![absDepPath].name).toBeTruthy()
  }

  await fs.mkdir('project-2')

  process.chdir('project-2')

  await addDependenciesToPackage(manifest, ['is-positive'], await testDefaults({
    save: true,
    lockfileDir: path.resolve('..'),
    storeDir,
    pruneLockfileImporters: false,
  }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

    expect(lockfile.importers).toHaveProperty(['project-2'])

    // previous entries are not removed
    const id = '/@pnpm.e2e/pkg-with-1-dep/100.0.0'

    expect(lockfile.importers.project.specifiers).toBeTruthy()
    expect(lockfile.importers.project.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')
    expect(lockfile.importers.project.dependencies).toHaveProperty(['@zkochan/foo'])
    expect(lockfile.importers.project.dependencies!['is-negative']).toContain('/')

    expect(lockfile.packages).toHaveProperty([id])
    expect(lockfile.packages![id].dependencies).toBeTruthy()
    expect(lockfile.packages![id].dependencies).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep'])
    expect(lockfile.packages![id].resolution).toHaveProperty(['integrity'])
    expect(lockfile.packages![id].resolution).not.toHaveProperty(['tarball'])

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    expect(lockfile.packages).toHaveProperty([absDepPath])
    expect(lockfile.packages![absDepPath].name).toBeTruthy()
  }
})

test(`doing named installation when shared ${WANTED_LOCKFILE} exists already`, async () => {
  const pkg1 = {
    name: 'pkg1',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^2.1.0',
    },
  }
  let pkg2: ProjectManifest = {
    name: 'pkg2',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^3.1.0',
    },
  }
  const projects = preparePackages([
    pkg1,
    pkg2,
  ])

  await writeYamlFile(WANTED_LOCKFILE, {
    importers: {
      pkg1: {
        dependencies: {
          'is-negative': '2.1.0',
        },
        specifiers: {
          'is-negative': '^2.1.0',
        },
      },
      pkg2: {
        dependencies: {
          'is-positive': '3.1.0',
        },
        specifiers: {
          'is-positive': '^3.1.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-negative/-/is-negative-2.1.0.tgz`,
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
  }, { lineWidth: 1000 })

  pkg2 = await addDependenciesToPackage(
    pkg2,
    ['is-positive'],
    await testDefaults({
      dir: path.resolve('pkg2'),
      lockfileDir: process.cwd(),
    })
  )

  const currentLockfile = await readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))

  expect(Object.keys(currentLockfile['importers'])).toStrictEqual(['pkg2'])

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest: pkg1,
        mutation: 'install',
        rootDir: path.resolve('pkg1'),
      },
      {
        buildIndex: 0,
        manifest: pkg2,
        mutation: 'install',
        rootDir: path.resolve('pkg2'),
      },
    ],
    await testDefaults()
  )

  await projects['pkg1'].has('is-negative')
  await projects['pkg2'].has('is-positive')
})

// Covers https://github.com/pnpm/pnpm/issues/1200
test(`use current ${WANTED_LOCKFILE} as initial wanted one, when wanted was removed`, async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['lodash@4.17.11', 'underscore@1.9.0'], await testDefaults())

  await rimraf(WANTED_LOCKFILE)

  await addDependenciesToPackage(manifest, ['underscore@1.9.1'], await testDefaults())

  await project.has('lodash')
  await project.has('underscore')
})

// Covers https://github.com/pnpm/pnpm/issues/1876
test('existing dependencies are preserved when updating a lockfile to a newer format', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults())

  const initialLockfile = await project.readLockfile()
  await writeYamlFile(WANTED_LOCKFILE, { ...initialLockfile, lockfileVersion: 5.01 }, { lineWidth: 1000 })

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  const updatedLockfile = await project.readLockfile()

  expect(initialLockfile.packages).toStrictEqual(updatedLockfile.packages)
})

test('lockfile is not getting broken if the used registry changes', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1'], await testDefaults())

  const newOpts = await testDefaults({ registries: { default: 'https://registry.npmjs.org/' } })
  let err!: PnpmError
  try {
    await addDependenciesToPackage(manifest, ['is-negative@1'], newOpts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_REGISTRIES_MISMATCH')

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], newOpts)
  await addDependenciesToPackage(manifest, ['is-negative@1'], newOpts)

  expect(Object.keys((await project.readLockfile()).packages)).toStrictEqual([
    '/is-negative/1.0.1',
    '/is-positive/1.0.0',
  ])
})

test('broken lockfile is fixed even if it seems like up to date at first. Unless frozenLockfile option is set to true', async () => {
  const project = prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults({ lockfileOnly: true }))
  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
    delete lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0']
    await writeYamlFile(WANTED_LOCKFILE, lockfile, { lineWidth: 1000 })
  }

  let err!: PnpmError
  try {
    await mutateModules([
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ], await testDefaults({ frozenLockfile: true }))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY')

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: true }))

  await project.has('@pnpm.e2e/pkg-with-1-dep')
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
})

const REGISTRY_MIRROR_DIR = path.join(__dirname, '../../../registry-mirror')

/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveMeta = loadJsonFile.sync<any>(path.join(REGISTRY_MIRROR_DIR, 'is-positive.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */
const tarballPath = path.join(REGISTRY_MIRROR_DIR, 'is-positive-3.1.0.tgz')

test('tarball domain differs from registry domain', async () => {
  nock('https://registry.example.com', { allowUnmocked: true })
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  nock('https://registry.npmjs.org', { allowUnmocked: true })
    .get('/is-positive/-/is-positive-3.1.0.tgz')
    .replyWithFile(200, tarballPath)

  const project = prepareEmpty()

  await addDependenciesToPackage({},
    [
      'is-positive',
    ], await testDefaults({
      fastUnpack: false,
      lockfileOnly: true,
      registries: {
        default: 'https://registry.example.com',
      },
      save: true,
    })
  )

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    dependencies: {
      'is-positive': 'registry.npmjs.org/is-positive/3.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'registry.npmjs.org/is-positive/3.1.0': {
        dev: false,
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          registry: 'https://registry.example.com/',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
        },
        version: '3.1.0',
      },
    },
    specifiers: { 'is-positive': '^3.1.0' },
  })
})

test('tarball installed through non-standard URL endpoint from the registry domain', async () => {
  nock('https://registry.npmjs.org', { allowUnmocked: true })
    .get('/is-positive/download/is-positive-3.1.0.tgz')
    .replyWithFile(200, tarballPath)

  const project = prepareEmpty()

  await addDependenciesToPackage({},
    [
      'https://registry.npmjs.org/is-positive/download/is-positive-3.1.0.tgz',
    ], await testDefaults({
      fastUnpack: false,
      lockfileOnly: true,
      registries: {
        default: 'https://registry.npmjs.org/',
      },
      save: true,
    })
  )

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    dependencies: {
      'is-positive': '@registry.npmjs.org/is-positive/download/is-positive-3.1.0.tgz',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@registry.npmjs.org/is-positive/download/is-positive-3.1.0.tgz': {
        dev: false,
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        resolution: {
          tarball: 'https://registry.npmjs.org/is-positive/download/is-positive-3.1.0.tgz',
        },
        version: '3.1.0',
      },
    },
    specifiers: {
      'is-positive': 'https://registry.npmjs.org/is-positive/download/is-positive-3.1.0.tgz',
    },
  })
})

test('a lockfile with merge conflicts is autofixed', async () => {
  const project = prepareEmpty()

  await fs.writeFile(WANTED_LOCKFILE, `\
importers:
  .:
    dependencies:
<<<<<<< HEAD
      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.0.0
=======
      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.1.0
>>>>>>> next
    specifiers:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '>100.0.0'
lockfileVersion: ${LOCKFILE_VERSION}
packages:
<<<<<<< HEAD
  /@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0:
    dev: false
    resolution:
      integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}
=======
  /@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0:
    dev: false
    resolution:
      integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')}
>>>>>>> next`, 'utf8')

  await install({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '>100.0.0',
    },
  }, await testDefaults())

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.1.0')
})

test('a lockfile with duplicate keys is fixed', async () => {
  const project = prepareEmpty()

  await fs.writeFile(WANTED_LOCKFILE, `\
importers:
  .:
    dependencies:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.0.0
    specifiers:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0'
lockfileVersion: ${LOCKFILE_VERSION}
packages:
  /@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0:
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
    dev: false
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
`, 'utf8')

  const reporter = jest.fn()
  await install({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
  }, await testDefaults({ reporter }))

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')

  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'warn',
    name: 'pnpm',
    prefix: process.cwd(),
    message: expect.stringMatching(/^Ignoring broken lockfile at .* duplicated mapping key/),
  }))
})

test('a lockfile with duplicate keys is causes an exception, when frozenLockfile is true', async () => {
  prepareEmpty()

  await fs.writeFile(WANTED_LOCKFILE, `\
importers:
  .:
    dependencies:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.0.0
    specifiers:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0'
lockfileVersion: ${LOCKFILE_VERSION}
packages:
  /@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0:
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
    dev: false
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
`, 'utf8')

  await expect(
    install({
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
    }, await testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(/^The lockfile at .* is broken: duplicated mapping key/)
})

test('a broken private lockfile is ignored', async () => {
  prepareEmpty()

  const manifest = await install({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
  }, await testDefaults())

  await fs.writeFile('node_modules/.pnpm/lock.yaml', `\
importers:
  .:
    dependencies:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.0.0
    specifiers:
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0'
lockfileVersion: ${LOCKFILE_VERSION}
packages:
  /@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0:
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
    dev: false
    resolution: {integrity: ${getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')}}
`, 'utf8')

  const reporter = jest.fn()

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      manifest,
      rootDir: process.cwd(),
    },
  ], await testDefaults({ reporter }))

  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'warn',
    name: 'pnpm',
    prefix: process.cwd(),
    message: expect.stringMatching(/^Ignoring broken lockfile at .* duplicated mapping key/),
  }))
})

// Covers https://github.com/pnpm/pnpm/issues/2928
test('build metadata is always ignored in versions and the lockfile is not flickering because of them', async () => {
  await addDistTag({ package: '@monorepolint/core', version: '0.5.0-alpha.51', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@monorepolint/cli@0.5.0-alpha.51',
    ], await testDefaults({ lockfileOnly: true }))

  const depPath = '/@monorepolint/core/0.5.0-alpha.51'
  const initialLockfile = await project.readLockfile()
  const initialPkgEntry = initialLockfile.packages[depPath]
  expect(initialPkgEntry?.resolution).toStrictEqual({
    integrity: 'sha512-ihFonHDppOZyG717OW6Bamd37mI2gQHjd09buTjbKhRX8NAHsTbRUKwp39ZYVI5AYgLF1eDlLpgOY4dHy2xGQw==',
  })

  await addDependenciesToPackage(manifest, ['is-positive'], await testDefaults({ lockfileOnly: true }))

  const updatedLockfile = await project.readLockfile()
  expect(initialPkgEntry).toStrictEqual(updatedLockfile.packages[depPath])
})

test('a broken lockfile should not break the store', async () => {
  prepareEmpty()
  const opts = await testDefaults()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], { ...opts, lockfileOnly: true })

  const lockfile: Lockfile = await readYamlFile(WANTED_LOCKFILE)
  lockfile.packages!['/is-positive/1.0.0'].name = 'bad-name'
  lockfile.packages!['/is-positive/1.0.0'].version = '1.0.0'

  await writeYamlFile(WANTED_LOCKFILE, lockfile)

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ lockfileOnly: true, storeDir: path.resolve('store2') }))

  delete lockfile.packages!['/is-positive/1.0.0'].name
  delete lockfile.packages!['/is-positive/1.0.0'].version

  await writeYamlFile(WANTED_LOCKFILE, lockfile)
  await rimraf(path.resolve('node_modules'))

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ lockfileOnly: true, storeDir: path.resolve('store2') }))
})

test('include tarball URL', async () => {
  const project = prepareEmpty()

  const opts = await testDefaults({ fastUnpack: false, lockfileIncludeTarballUrl: true })
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)

  const lockfile = await project.readLockfile()
  expect((lockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution as TarballResolution).tarball)
    .toBe(`http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e%2fpkg-with-1-dep/-/pkg-with-1-dep-100.0.0.tgz`)
})
