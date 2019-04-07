import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { PackageJson } from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import path = require('path')
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writePkg = require('write-pkg')
import {
  addDistTag,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('save to package.json (rimraf@2.5.1)', async (t) => {
  const project = prepare(t)
  await addDependenciesToPackage(['rimraf@^2.5.1'], await testDefaults({ save: true }))

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')

  const pkgJson = await readPackageJsonFromDir(process.cwd())
  t.deepEqual(pkgJson.dependencies, { rimraf: '^2.5.1' }, 'rimraf has been added to dependencies')
})

// NOTE: this works differently for global installations. See similar tests in global.ts
test("don't override existing spec in package.json on named installation", async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '^1.0.0', // this will be updated
      'is-positive': '^2.0.0', // this will be kept as no newer version is available from the range
      'sec': 'sindresorhus/sec',
    },
  })
  await addDependenciesToPackage(['is-positive'], await testDefaults())
  await addDependenciesToPackage(['is-negative'], await testDefaults())
  await addDependenciesToPackage(['sec'], await testDefaults())

  t.equal(project.requireModule('is-positive/package.json').version, '2.0.0')
  t.equal(project.requireModule('is-negative/package.json').version, '1.0.1')

  const pkgJson = await loadJsonFile<PackageJson>(path.resolve('package.json'))
  t.deepEqual(pkgJson.dependencies, {
    'is-negative': '^1.0.1',
    'is-positive': '^2.0.0',
    'sec': 'github:sindresorhus/sec',
  })
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async (t) => {
  const project = prepare(t)
  await addDependenciesToPackage(['@rstacruz/tap-spec'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  const m = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m === 'function', 'tapSpec() is available')

  const pkgJson = await readPackageJsonFromDir(process.cwd())
  t.deepEqual(pkgJson.devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')
})

test('dependency should not be added to package.json if it is already there', async (t: tape.Test) => {
  await addDistTag('foo', '100.0.0', 'latest')
  await addDistTag('bar', '100.0.0', 'latest')

  const project = prepare(t, {
    devDependencies: {
      foo: '^100.0.0',
    },
    optionalDependencies: {
      bar: '^100.0.0',
    },
  })
  await addDependenciesToPackage(['foo', 'bar'], await testDefaults())

  const pkgJson = await loadJsonFile(path.resolve('package.json'))
  t.deepEqual(pkgJson, {
    devDependencies: {
      foo: '^100.0.0',
    },
    name: 'project',
    optionalDependencies: {
      bar: '^100.0.0',
    },
    version: '0.0.0',
  }, 'package.json was not changed')

  const lockfile = await project.loadLockfile()

  t.equal(lockfile.devDependencies.foo, '100.0.0', `\`foo\` is in the devDependencies property of ${WANTED_LOCKFILE}`)
  t.ok(lockfile.packages['/foo/100.0.0'].dev, `the \`foo\` package is marked as dev in ${WANTED_LOCKFILE}`)

  t.equal(lockfile.optionalDependencies.bar, '100.0.0', `\`bar\` is in the optionalDependencies property of ${WANTED_LOCKFILE}`)
  t.ok(lockfile.packages['/bar/100.0.0'].optional, `the \`bar\` package is marked as optional in ${WANTED_LOCKFILE}`)
})

test('dependencies should be updated in the fields where they already are', async (t: tape.Test) => {
  await addDistTag('foo', '100.1.0', 'latest')
  await addDistTag('bar', '100.1.0', 'latest')

  const project = prepare(t, {
    devDependencies: {
      foo: '^100.0.0',
    },
    optionalDependencies: {
      bar: '^100.0.0',
    },
  })
  await addDependenciesToPackage(['foo@latest', 'bar@latest'], await testDefaults())

  const pkgJson = await loadJsonFile(path.resolve('package.json'))
  t.deepEqual(pkgJson, {
    devDependencies: {
      foo: '^100.1.0',
    },
    name: 'project',
    optionalDependencies: {
      bar: '^100.1.0',
    },
    version: '0.0.0',
  }, 'package.json updated dependencies in the correct properties')
})

test('dependency should be removed from the old field when installing it as a different type of dependency', async (t: tape.Test) => {
  await addDistTag('foo', '100.0.0', 'latest')
  await addDistTag('bar', '100.0.0', 'latest')
  await addDistTag('qar', '100.0.0', 'latest')

  const project = prepare(t, {
    dependencies: {
      foo: '^100.0.0',
    },
    devDependencies: {
      bar: '^100.0.0',
    },
    optionalDependencies: {
      qar: '^100.0.0',
    },
  })
  await addDependenciesToPackage(['foo'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await addDependenciesToPackage(['bar'], await testDefaults({ targetDependenciesField: 'dependencies' }))
  await addDependenciesToPackage(['qar'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  {
    const pkgJson = await loadJsonFile(path.resolve('package.json'))
    t.deepEqual(pkgJson, {
      dependencies: {
        bar: '^100.0.0',
      },
      devDependencies: {
        qar: '^100.0.0',
      },
      name: 'project',
      optionalDependencies: {
        foo: '^100.0.0',
      },
      version: '0.0.0',
    }, 'dependencies moved around correctly')
  }

  await addDependenciesToPackage(['bar', 'foo', 'qar'], await testDefaults({ targetDependenciesField: 'dependencies' }))

  {
    const pkgJson = await loadJsonFile(path.resolve('package.json'))
    t.deepEqual(pkgJson, {
      dependencies: {
        bar: '^100.0.0',
        foo: '^100.0.0',
        qar: '^100.0.0',
      },
      name: 'project',
      version: '0.0.0',
    }, `dependencies moved around correctly when installed with node_modules and ${WANTED_LOCKFILE} present`)
    const lockfile = await project.loadCurrentLockfile()
    t.deepEqual(Object.keys(lockfile.dependencies), ['bar', 'foo', 'qar'], 'lockfile updated')
  }

  {
    t.comment('manually editing package.json. Converting all prod deps to dev deps')

    const pkgJson = await loadJsonFile<PackageJson>(path.resolve('package.json'))
    pkgJson.devDependencies = pkgJson.dependencies
    delete pkgJson.dependencies
    await writePkg(pkgJson)

    await install(await testDefaults())

    const lockfile = await project.loadCurrentLockfile()
    t.deepEqual(Object.keys(lockfile.devDependencies), ['bar', 'foo', 'qar'], 'lockfile updated')
    t.notOk(lockfile.dependencies)
  }
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], await testDefaults({ save: true, pinnedVersion: 'patch' }))

  const m1 = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m1 === 'function', 'tapSpec() is available')

  const m2 = project.requireModule('rimraf')
  t.ok(typeof m2 === 'function', 'rimraf() is available')

  const pkgJson = await readPackageJsonFromDir(process.cwd())
  const expectedDeps = {
    '@rstacruz/tap-spec': '4.1.1',
    'rimraf': '2.5.1',
  }
  t.deepEqual(pkgJson.dependencies, expectedDeps, 'tap-spec and rimraf have been added to dependencies')
  t.deepEqual(Object.keys(pkgJson.dependencies!), Object.keys(expectedDeps), 'tap-spec and rimraf have been added to dependencies in sorted order')
})

test('save to package.json with save prefix ~', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['pkg-with-1-dep'], await testDefaults({ pinnedVersion: 'minor' }))

  const pkgJson = await readPackageJsonFromDir(process.cwd())
  t.deepEqual(pkgJson.dependencies, { 'pkg-with-1-dep': '~100.0.0' }, 'rimraf have been added to dependencies')
})
