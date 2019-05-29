import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import deepRequireCwd = require('deep-require-cwd')
import loadJsonFile = require('load-json-file')
import makeDir = require('make-dir')
import path = require('path')
import exists = require('path-exists')
import { addDistTag } from 'pnpm-registry-mock'
import readYamlFile from 'read-yaml-file'
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  uninstall,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
const NM = 'node_modules'

test("don't fail when peer dependency is fetched from GitHub", async (t) => {
  prepareEmpty(t)
  await addDependenciesToPackage({}, ['test-pnpm-peer-deps'], await testDefaults())
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults()
  let manifest = await addDependenciesToPackage({}, ['using-ajv'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  const storeIndex = await loadJsonFile<object>(path.join(opts.store, 'store.json'))
  t.ok(storeIndex['localhost+4873/ajv-keywords/1.5.0'], 'localhost+4873/ajv-keywords/1.5.0 added to store index')
  t.ok(storeIndex['localhost+4873/using-ajv/1.0.0'], 'localhost+4873/using-ajv/1.0.0 added to store index')

  // testing that peers are reinstalled correctly using info from the lockfile
  await rimraf('node_modules')
  await rimraf(path.resolve('..', '.store'))
  manifest = await install(manifest, await testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  await addDependenciesToPackage(manifest, ['using-ajv'], await testDefaults({ update: true }))

  const lockfile = await project.readLockfile()

  t.equal(
    lockfile.packages['/using-ajv/1.0.0'].dependencies['ajv-keywords'],
    '1.5.0_ajv@4.10.4',
    `${WANTED_LOCKFILE}: correct reference is created to ajv-keywords from using-ajv`,
  )
  // covers https://github.com/pnpm/pnpm/issues/1150
  t.ok(lockfile.packages['/ajv-keywords/1.5.0_ajv@4.10.4'])
})

// Covers https://github.com/pnpm/pnpm/issues/1133
test('nothing is needlessly removed from node_modules', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults()
  const manifest = await addDependenciesToPackage({}, ['using-ajv', 'ajv-keywords@1.5.0'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'root dependency resolution is present')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  await uninstall(manifest, ['ajv-keywords'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency link is not removed')
  t.notOk(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'root dependency resolution is removed')
})

test('peer dependency is grouped with dependent when the peer is a top dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch({
    message: 'localhost+4873/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  }), 'no warning is logged about unresolved peer dep')

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv-keywords')), 'dependent is grouped with top peer dep')
})

test('warning is reported when cannot resolve peer dependency for top-level dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('strict-peer-dependencies: error is thrown when cannot resolve peer dependency for top-level dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage({}, ['ajv-keywords@1.5.0'], await testDefaults({ reporter, strictPeerDependencies: true }))
  } catch (_) {
    err = _
  }

  t.ok(err, 'error is thrown')
  t.equal(err.code, 'ERR_PNPM_MISSING_PEER_DEPENDENCY')
  t.equal(err.message, 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.')
})

test('warning is not reported if the peer dependency can be required from a node_modules of a parent directory', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.0'], await testDefaults())

  await makeDir('pkg')

  process.chdir('pkg')

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 0, 'warning is not logged')
})

test('warning is reported when cannot resolve peer dependency for non-top-level dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['abc-grand-parent-without-c'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('warning is reported when bad version of resolved peer dependency for non-top-level dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but version 2.0.0 was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('strict-peer-dependencies: error is thrown when bad version of resolved peer dependency for non-top-level dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage({}, ['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ reporter, strictPeerDependencies: true }))
  } catch (_) {
    err = _
  }

  t.ok(err, 'error is thrown')
  t.equal(err.code, 'ERR_PNPM_INVALID_PEER_DEPENDENCY')
  t.equal(err.message, 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but version 2.0.0 was installed.')
})

test('top peer dependency is linked on subsequent install', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4'], await testDefaults())

  await addDependenciesToPackage(manifest, ['ajv-keywords@1.5.0'], await testDefaults())

  t.notOk(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'dependency without peer is prunned')
  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
})

async function okFile (t: tape.Test, filename: string) {
  t.ok(await exists(filename), `exists ${filename}`)
}

// This usecase was failing. See https://github.com/pnpm/supi/issues/15
test('peer dependencies are linked when running one named installation', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c', 'abc-parent-with-ab', 'peer-c@2.0.0'], await testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_f101cfec1621b915239e5c82246da43c', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'peer-c'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')

  // this part was failing. See issue: https://github.com/pnpm/pnpm/issues/1201
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ update: true, depth: 100 }))
})

test('peer dependencies are linked when running two separate named installations', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c', 'peer-c@2.0.0'], await testDefaults())
  await addDependenciesToPackage(manifest, ['abc-parent-with-ab'], await testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')
})

// tslint:disable-next-line:no-string-literal
test['skip']('peer dependencies are linked', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await install({
    dependencies: {
      'abc-grand-parent-with-c': '*',
      'peer-c': '2.0.0',
    },
    devDependencies: {
      'abc-parent-with-ab': '*',
    },
  }, await testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir, '165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/abc-parent-with-ab/1.0.0/peer-a@1.0.0+peer-b@1.0.0'].dev, `the dev resolution set is marked as dev in ${WANTED_LOCKFILE}`)
})

test('scoped peer dependency is linked', async (t: tape.Test) => {
  prepareEmpty(t)
  await addDependenciesToPackage({}, ['for-testing-scoped-peers'], await testDefaults())

  const pkgVariation = path.join(NM, '.localhost+4873', '@having', 'scoped-peer', '1.0.0_@scoped+peer@1.0.0', NM)
  await okFile(t, path.join(pkgVariation, '@having', 'scoped-peer'))
  await okFile(t, path.join(pkgVariation, '@scoped', 'peer'))
})

test('peer bins are linked', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['for-testing-peers-having-bins'], await testDefaults())

  const pkgVariation = path.join('.localhost+4873', 'pkg-with-peer-having-bin', '1.0.0_peer-with-bin@1.0.0', NM)

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'peer-with-bin'))

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty(t)
  await addDependenciesToPackage({}, ['parent-of-pkg-with-events-and-peers', 'pkg-with-events-and-peers', 'peer-c@2.0.0'], await testDefaults())

  const pkgVariation1 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0_peer-c@1.0.0', NM)
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))

  const pkgVariation2 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0_peer-c@2.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))
})

test('package that resolves its own peer dependency', async (t: tape.Test) => {
  // TODO: investigate how npm behaves in such situations
  // should there be a warning printed?
  // does it currently print a warning that peer dependency is not resolved?

  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['pkg-with-resolved-peer', 'peer-c@2.0.0'], await testDefaults())

  t.equal(deepRequireCwd(['pkg-with-resolved-peer', 'peer-c', './package.json']).version, '1.0.0')

  t.ok(await exists(path.join(NM, '.localhost+4873', 'pkg-with-resolved-peer', '1.0.0', NM, 'pkg-with-resolved-peer')))

  const lockfile = await project.readLockfile()

  t.notOk(lockfile.packages['/pkg-with-resolved-peer/1.0.0'].peerDependencies, 'peerDependencies not added to lockfile')
  t.ok(lockfile.packages['/pkg-with-resolved-peer/1.0.0'].dependencies['peer-c'])
  t.ok(lockfile.packages['/pkg-with-resolved-peer/1.0.0'].optionalDependencies['peer-b'])
})

test('package that has parent as peer dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['has-alpha', 'alpha'], await testDefaults())

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/has-alpha-as-peer/1.0.0_alpha@1.0.0'])
  t.notOk(lockfile.packages['/has-alpha-as-peer/1.0.0'])
})

test('own peer installed in root as well is linked to root', async (t: tape.Test) => {
  prepareEmpty(t)

  await addDependenciesToPackage({}, ['is-negative@kevva/is-negative#2.1.0', 'peer-deps-in-child-pkg'], await testDefaults())

  t.ok(deepRequireCwd.silent(['is-negative', './package.json']), 'is-negative is linked to root')
})

test('peer dependency is grouped with dependent when the peer is a top dependency but an external lockfile is used', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter, lockfileDirectory: path.resolve('..') }))

  t.notOk(reporter.calledWithMatch({
    message: 'localhost+4873/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  }), 'no warning is logged about unresolved peer dep')

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv-keywords')))

  const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

  t.deepEqual(lockfile['importers']['project'], { // tslint:disable-line
    dependencies: {
      'ajv': '4.10.4',
      'ajv-keywords': '1.5.0_ajv@4.10.4',
    },
    specifiers: {
      'ajv': '4.10.4',
      'ajv-keywords': '1.5.0',
    },
  }, `correct ${WANTED_LOCKFILE} created`)
})

// Covers https://github.com/pnpm/pnpm/issues/1483
test('peer dependency is grouped correctly with peer installed via separate installation when external lockfile is used', async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()
  const lockfileDirectory = path.resolve('..')

  const manifest = await install({
    dependencies: {
      'abc': '1.0.0',
    },
  }, await testDefaults({ reporter, lockfileDirectory }))
  await addDependenciesToPackage(manifest, ['peer-c@2.0.0'], await testDefaults({ reporter, lockfileDirectory }))

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'abc', '1.0.0_peer-c@2.0.0', NM, 'dep-of-pkg-with-1-dep')))
})

test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async (t: tape.Test) => {
  prepareEmpty(t)
  await makeDir('_')
  process.chdir('_')
  const lockfileDirectory = path.resolve('..')

  let manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }

  manifest = await install(manifest, await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }

  // Covers https://github.com/pnpm/pnpm/issues/1506
  await mutateModules(
    [
      {
        dependencyNames: ['ajv'],
        manifest,
        mutation: 'uninstallSome',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({
      lockfileDirectory,
    }),
  )

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'ajv-keywords': '1.5.0',
      },
      specifiers: {
        'ajv-keywords': '1.5.0',
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update', async (t: tape.Test) => {
  prepareEmpty(t)
  await makeDir('_')
  process.chdir('_')
  const lockfileDirectory = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.4.0'], await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.4.0_ajv@4.10.4',
      },
      specifiers: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.4.0',
      },
    })
  }

  await addDependenciesToPackage(manifest, ['ajv-keywords@1.5.0'], await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        'ajv': '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update of the resolved package', async (t: tape.Test) => {
  prepareEmpty(t)
  await makeDir('_')
  process.chdir('_')
  const lockfileDirectory = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['peer-c@1.0.0', 'abc-parent-with-ab@1.0.0'], await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'abc-parent-with-ab': '1.0.0_peer-c@1.0.0',
        'peer-c': '1.0.0',
      },
      specifiers: {
        'abc-parent-with-ab': '1.0.0',
        'peer-c': '1.0.0',
      },
    })
  }

  await addDependenciesToPackage(manifest, ['peer-c@2.0.0'], await testDefaults({ lockfileDirectory }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    t.deepEqual(lockfile['importers']['_'], {
      dependencies: {
        'abc-parent-with-ab': '1.0.0_peer-c@2.0.0',
        'peer-c': '2.0.0',
      },
      specifiers: {
        'abc-parent-with-ab': '1.0.0',
        'peer-c': '2.0.0',
      },
    })
  }

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'abc-parent-with-ab', '1.0.0_peer-c@2.0.0', NM, 'is-positive')))
})

test('regular dependencies are not removed on update from transitive packages that have children with peers resolved from above', async (t: tape.Test) => {
  prepareEmpty(t)
  await makeDir('_')
  process.chdir('_')
  const lockfileDirectory = path.resolve('..')
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c@1.0.0'], await testDefaults({ lockfileDirectory }))

  await addDistTag({ package: 'peer-c', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ lockfileDirectory, update: true, depth: 2 }))

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'abc-parent-with-ab', '1.0.1_peer-c@1.0.1', NM, 'is-positive')))
})

test('peer dependency is resolved from parent package', async (t) => {
  preparePackages(t, [
    {
      name: 'pkg',
    }
  ])
  await mutateModules([
    {
      dependencySelectors: ['tango@1.0.0'],
      manifest: {},
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  t.deepEqual(Object.keys(lockfile.packages || {}), [
    '/has-tango-as-peer-dep/1.0.0_tango@1.0.0',
    '/tango/1.0.0_tango@1.0.0',
  ])
})

test('transitive peerDependencies field does not break the lockfile on subsequent named install', async (t) => {
  preparePackages(t, [
    {
      name: 'pkg',
    }
  ])
  const [{ manifest }] = await mutateModules([
    {
      dependencySelectors: ['most@1.7.3'],
      manifest: {},
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  await mutateModules([
    {
      dependencySelectors: ['is-positive'],
      manifest,
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  t.deepEqual(Object.keys(lockfile.packages!['/most/1.7.3_most@1.7.3'].dependencies!), [
    '@most/multicast',
    '@most/prelude',
    'symbol-observable'
  ])
})

test('peer dependency is resolved from parent package via its alias', async (t) => {
  preparePackages(t, [
    {
      name: 'pkg',
    }
  ])
  await mutateModules([
    {
      dependencySelectors: ['tango@npm:tango-tango@1.0.0'],
      manifest: {},
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  t.deepEqual(Object.keys(lockfile.packages || {}), [
    '/has-tango-as-peer-dep/1.0.0_tango@1.0.0',
    '/tango-tango/1.0.0_tango@1.0.0',
  ])
})

test('peer dependency is saved', async (t) => {
  prepareEmpty(t)

  let manifest = await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    await testDefaults({
      peer: true,
      targetDependenciesField: 'devDependencies',
    }),
  )

  t.deepEqual(
    manifest,
    {
      devDependencies: { 'is-positive': '1.0.0' },
      peerDependencies: { 'is-positive': '1.0.0' },
    },
  )

  manifest = await uninstall(manifest, ['is-positive'], await testDefaults())

  t.deepEqual(
    manifest,
    {
      devDependencies: {},
      peerDependencies: {},
    },
  )
})

test('warning is not reported when cannot resolve optional peer dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['abc-optional-peers@1.0.0', 'peer-c@2.0.0'], await testDefaults({ reporter }))

  {
    const logMatcher = sinon.match({
      message: 'abc-optional-peers@1.0.0 requires a peer of peer-a@^1.0.0 but none was installed.',
    })
    const reportedTimes = reporter.withArgs(logMatcher).callCount

    t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
  }

  {
    const logMatcher = sinon.match({
      message: 'abc-optional-peers@1.0.0 requires a peer of peer-b@^1.0.0 but none was installed.',
    })
    const reportedTimes = reporter.withArgs(logMatcher).callCount

    t.equal(reportedTimes, 0, 'warning is not logged about unresolved optional peer dep')
  }

  {
    const logMatcher = sinon.match({
      message: 'abc-optional-peers@1.0.0 requires a peer of peer-c@^1.0.0 but version 2.0.0 was installed.',
    })
    const reportedTimes = reporter.withArgs(logMatcher).callCount

    t.equal(reportedTimes, 1, 'warning is logged bad version number of optional peer dep')
  }

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile.packages['/abc-optional-peers/1.0.0_peer-c@2.0.0'].peerDependenciesMeta, {
    'peer-b': {
      optional: true,
    },
    'peer-c': {
      optional: true,
    },
  })
})
