import tape = require('tape')
import promisifyTape from 'tape-promise'
import {installPkgs, install} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import rimraf = require('rimraf-then')
import exists = require('path-exists')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)

test('run pre/postinstall scripts', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults({saveDev: true}))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in shrinkwrap
  // and that their scripts are not tried to be executed

  await install(testDefaults({production: true}))

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is not available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is not available')
  }
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async function (t: tape.Test) {
  const project = prepare(t)

  await installPkgs(['hello-world-js-bin'], testDefaults())
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults({saveDev: true}))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async function (t) {
  const project = prepare(t)
  await installPkgs(['install-script-example'], testDefaults())

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('run install scripts in the current project', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
    }
  })
  await installPkgs(['json-append@1.1.1'], testDefaults())
  await install(testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('installation fails if lifecycle script fails', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'exit 1'
    },
  })

  try {
    await install(testDefaults())
    t.fail('should have failed')
  } catch (err) {
    t.equal(err['code'], 'ELIFECYCLE', 'failed with correct error code')
  }
})
