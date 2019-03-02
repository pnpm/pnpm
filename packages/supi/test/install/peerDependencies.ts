import prepare, { preparePackages } from '@pnpm/prepare'
import { Shrinkwrap } from '@pnpm/shrinkwrap-file'
import deepRequireCwd = require('deep-require-cwd')
import loadJsonFile from 'load-json-file'
import mkdir = require('mkdirp-promise')
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
  const project = prepare(t)
  await addDependenciesToPackage(['test-pnpm-peer-deps'], await testDefaults())
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults()
  await addDependenciesToPackage(['using-ajv'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  const storeIndex = await loadJsonFile<object>(path.join(opts.store, 'store.json'))
  t.ok(storeIndex['localhost+4873/ajv-keywords/1.5.0'], 'localhost+4873/ajv-keywords/1.5.0 added to store index')
  t.ok(storeIndex['localhost+4873/using-ajv/1.0.0'], 'localhost+4873/using-ajv/1.0.0 added to store index')

  // testing that peers are reinstalled correctly using info from the shrinkwrap file
  await rimraf('node_modules')
  await rimraf(path.resolve('..', '.store'))
  await install(await testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  await addDependenciesToPackage(['using-ajv'], await testDefaults({ update: true }))

  const shr = await project.loadShrinkwrap()

  t.equal(
    shr.packages['/using-ajv/1.0.0'].dependencies['ajv-keywords'],
    '1.5.0_ajv@4.10.4',
    'shrinkwrap.yaml: correct reference is created to ajv-keywords from using-ajv',
  )
  // covers https://github.com/pnpm/pnpm/issues/1150
  t.ok(shr.packages['/ajv-keywords/1.5.0_ajv@4.10.4'])
})

// Covers https://github.com/pnpm/pnpm/issues/1133
test('nothing is needlessly removed from node_modules', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults()
  await addDependenciesToPackage(['using-ajv', 'ajv-keywords@1.5.0'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'root dependency resolution is present')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')

  await uninstall(['ajv-keywords'], opts)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency link is not removed')
  t.notOk(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'root dependency resolution is removed')
})

test('peer dependency is not grouped with dependent when the peer is a top dependency', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch({
    message: 'localhost+4873/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  }), 'no warning is logged about unresolved peer dep')

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'dependent is at the normal location')
})

test('warning is reported when cannot resolve peer dependency for top-level dependency', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('strict-peer-dependencies: error is thrown when cannot resolve peer dependency for top-level dependency', async (t: tape.Test) => {
  prepare(t)

  const reporter = sinon.spy()

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage(['ajv-keywords@1.5.0'], await testDefaults({ reporter, strictPeerDependencies: true }))
  } catch (_) {
    err = _
  }

  t.ok(err, 'error is thrown')
  t.equal(err.code, 'ERR_PNPM_MISSING_PEER_DEPENDENCY')
  t.equal(err.message, 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.')
})

test('warning is not reported if the peer dependency can be required from a node_modules of a parent directory', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['ajv@4.10.0'], await testDefaults())

  await mkdir('pkg')

  process.chdir('pkg')

  const reporter = sinon.spy()

  await addDependenciesToPackage(['ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 0, 'warning is not logged')
})

test('warning is reported when cannot resolve peer dependency for non-top-level dependency', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['abc-grand-parent-without-c'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but none was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('warning is reported when bad version of resolved peer dependency for non-top-level dependency', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ reporter }))

  const logMatcher = sinon.match({
    message: 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but version 2.0.0 was installed.',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount

  t.equal(reportedTimes, 1, 'warning is logged (once) about unresolved peer dep')
})

test('strict-peer-dependencies: error is thrown when bad version of resolved peer dependency for non-top-level dependency', async (t: tape.Test) => {
  prepare(t)

  const reporter = sinon.spy()

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage(['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ reporter, strictPeerDependencies: true }))
  } catch (_) {
    err = _
  }

  t.ok(err, 'error is thrown')
  t.equal(err.code, 'ERR_PNPM_INVALID_PEER_DEPENDENCY')
  t.equal(err.message, 'abc-grand-parent-without-c > abc-parent-with-ab: abc@1.0.0 requires a peer of peer-c@^1.0.0 but version 2.0.0 was installed.')
})

test('top peer dependency is not linked on subsequent install', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['ajv@4.10.4'], await testDefaults())

  await addDependenciesToPackage(['ajv-keywords@1.5.0'], await testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'dependent is at the normal location')
  t.notOk(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv')), 'peer dependency is not linked')
})

async function okFile (t: tape.Test, filename: string) {
  t.ok(await exists(filename), `exists ${filename}`)
}

// This usecase was failing. See https://github.com/pnpm/supi/issues/15
test('peer dependencies are linked when running one named installation', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  const project = prepare(t)

  await addDependenciesToPackage(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'peer-c@2.0.0'], await testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_peer-a@1.0.0+peer-b@1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')

  // this part was failing. See issue: https://github.com/pnpm/pnpm/issues/1201
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await install(await testDefaults({ update: true, depth: 100 }))
})

test('peer dependencies are linked when running two separate named installations', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepare(t)

  await addDependenciesToPackage(['abc-grand-parent-with-c', 'peer-c@2.0.0'], await testDefaults())
  await addDependenciesToPackage(['abc-parent-with-ab'], await testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_peer-a@1.0.0+peer-b@1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')
})

// tslint:disable-next-line:no-string-literal
test['skip']('peer dependencies are linked', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'abc-grand-parent-with-c': '*',
      'peer-c': '2.0.0',
    },
    devDependencies: {
      'abc-parent-with-ab': '*',
    },
  })
  await install(await testDefaults())

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

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/abc-parent-with-ab/1.0.0/peer-a@1.0.0+peer-b@1.0.0'].dev, 'the dev resolution set is marked as dev in shrinkwrap.yaml')
})

test('scoped peer dependency is linked', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['for-testing-scoped-peers'], await testDefaults())

  const pkgVariation = path.join(NM, '.localhost+4873', '@having', 'scoped-peer', '1.0.0_@scoped+peer@1.0.0', NM)
  await okFile(t, path.join(pkgVariation, '@having', 'scoped-peer'))
  await okFile(t, path.join(pkgVariation, '@scoped', 'peer'))
})

test('peer bins are linked', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['for-testing-peers-having-bins'], await testDefaults())

  const pkgVariation = path.join('.localhost+4873', 'pkg-with-peer-having-bin', '1.0.0_peer-with-bin@1.0.0', NM)

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'peer-with-bin'))

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async (t: tape.Test) => {
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepare(t)
  await addDependenciesToPackage(['parent-of-pkg-with-events-and-peers', 'pkg-with-events-and-peers', 'peer-c@2.0.0'], await testDefaults())

  const pkgVariation1 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0_peer-c@1.0.0', NM)
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))

  const pkgVariation2 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))
})

test('package that resolves its own peer dependency', async (t: tape.Test) => {
  // TODO: investigate how npm behaves in such situations
  // should there be a warning printed?
  // does it currently print a warning that peer dependency is not resolved?

  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepare(t)
  await addDependenciesToPackage(['pkg-with-resolved-peer', 'peer-c@2.0.0'], await testDefaults())

  t.equal(deepRequireCwd(['pkg-with-resolved-peer', 'peer-c', './package.json']).version, '1.0.0')

  t.ok(await exists(path.join(NM, '.localhost+4873', 'pkg-with-resolved-peer', '1.0.0', NM, 'pkg-with-resolved-peer')))

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/pkg-with-resolved-peer/1.0.0'].peerDependencies, 'peerDependencies not added to shrinkwrap')
  t.ok(shr.packages['/pkg-with-resolved-peer/1.0.0'].dependencies['peer-c'])
  t.ok(shr.packages['/pkg-with-resolved-peer/1.0.0'].optionalDependencies['peer-b'])
})

test('package that has parent as peer dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['has-alpha', 'alpha'], await testDefaults())

  const shr = await project.loadShrinkwrap()

  t.ok(shr.packages['/has-alpha-as-peer/1.0.0_alpha@1.0.0'])
  t.ok(shr.packages['/has-alpha-as-peer/1.0.0'])
})

test('own peer installed in root as well is linked to root', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-negative@kevva/is-negative#2.1.0', 'peer-deps-in-child-pkg'], await testDefaults())

  t.ok(deepRequireCwd.silent(['is-negative', './package.json']), 'is-negative is linked to root')
})

test('peer dependency is grouped with dependent when the peer is a top dependency but an external shrinkwrap is used', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter, shrinkwrapDirectory: path.resolve('..') }))

  t.notOk(reporter.calledWithMatch({
    message: 'localhost+4873/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.',
  }), 'no warning is logged about unresolved peer dep')

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'ajv-keywords', '1.5.0_ajv@4.10.4', NM, 'ajv-keywords')))

  const shr = await readYamlFile<Shrinkwrap>(path.join('..', 'shrinkwrap.yaml'))

  t.deepEqual(shr['importers']['project'], { // tslint:disable-line
    dependencies: {
      'ajv': '4.10.4',
      'ajv-keywords': '1.5.0_ajv@4.10.4',
    },
    specifiers: {
      'ajv': '4.10.4',
      'ajv-keywords': '1.5.0',
    },
  }, 'correct shrinkwrap.yaml created')
})

// Covers https://github.com/pnpm/pnpm/issues/1483
test('peer dependency is grouped correctly with peer installed via separate installation when external shrinkwrap is used', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'abc': '1.0.0',
    },
  })

  const reporter = sinon.spy()
  const shrinkwrapDirectory = path.resolve('..')

  await install(await testDefaults({ reporter, shrinkwrapDirectory }))
  await addDependenciesToPackage(['peer-c@2.0.0'], await testDefaults({ reporter, shrinkwrapDirectory }))

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'abc', '1.0.0_peer-c@2.0.0', NM, 'dep-of-pkg-with-1-dep')))
})

test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async (t: tape.Test) => {
  const project = prepare(t)
  await mkdir('_')
  process.chdir('_')
  const shrinkwrapDirectory = path.resolve('..')

  await addDependenciesToPackage(['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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

  await install(await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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
        mutation: 'uninstallSome',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({
      shrinkwrapDirectory,
    }),
  )

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
      dependencies: {
        'ajv-keywords': '1.5.0',
      },
      specifiers: {
        'ajv-keywords': '1.5.0',
      },
    })
  }
})

test('external shrinkwrap: peer dependency is grouped with dependent even after a named update', async (t: tape.Test) => {
  const project = prepare(t)
  await mkdir('_')
  process.chdir('_')
  const shrinkwrapDirectory = path.resolve('..')

  await addDependenciesToPackage(['ajv@4.10.4', 'ajv-keywords@1.4.0'], await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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

  await addDependenciesToPackage(['ajv-keywords@1.5.0'], await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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

test('external shrinkwrap: peer dependency is grouped with dependent even after a named update of the resolved package', async (t: tape.Test) => {
  const project = prepare(t)
  await mkdir('_')
  process.chdir('_')
  const shrinkwrapDirectory = path.resolve('..')

  await addDependenciesToPackage(['peer-c@1.0.0', 'abc-parent-with-ab@1.0.0'], await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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

  await addDependenciesToPackage(['peer-c@2.0.0'], await testDefaults({ shrinkwrapDirectory }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))
    t.deepEqual(shr['importers']['_'], {
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
  const project = prepare(t)
  await mkdir('_')
  process.chdir('_')
  const shrinkwrapDirectory = path.resolve('..')
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  await addDependenciesToPackage(['abc-grand-parent-with-c@1.0.0'], await testDefaults({ shrinkwrapDirectory }))

  await addDistTag({ package: 'peer-c', version: '1.0.1', distTag: 'latest' })
  await install(await testDefaults({ shrinkwrapDirectory, update: true, depth: 2 }))

  t.ok(await exists(path.join('..', NM, '.localhost+4873', 'abc-parent-with-ab', '1.0.1_peer-c@1.0.1', NM, 'is-positive')))
})

test('peer dependency is resolved from parent package', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'pkg',
    }
  ])
  await mutateModules([
    {
      dependencySelectors: ['tango@1.0.0'],
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
  t.deepEqual(Object.keys(shr.packages || {}), [
    '/has-tango-as-peer-dep/1.0.0_tango@1.0.0',
    '/tango/1.0.0_tango@1.0.0',
  ])
})

test('peer dependency is resolved from parent package via its alias', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'pkg',
    }
  ])
  await mutateModules([
    {
      dependencySelectors: ['tango@npm:tango-tango@1.0.0'],
      mutation: 'installSome',
      prefix: path.resolve('pkg'),
    },
  ], await testDefaults())

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
  t.deepEqual(Object.keys(shr.packages || {}), [
    '/has-tango-as-peer-dep/1.0.0_tango@1.0.0',
    '/tango-tango/1.0.0_tango@1.0.0',
  ])
})
