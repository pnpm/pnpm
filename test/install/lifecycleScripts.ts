import tape = require('tape')
import promisifyTape from 'tape-promise'
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
  execPnpmSync,
} from '../utils'

const test = promisifyTape(tape)

test('run pre/postinstall scripts', async function (t) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults())

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

test('preinstall is executed before general installation', t => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'preinstall script was executed')

  t.end()
})

test('postinstall is executed after general installation', t => {
  const project = prepare(t, {
    scripts: {
      postinstall: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('postinstall is not executed after named installation', t => {
  const project = prepare(t, {
    scripts: {
      postinstall: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install', 'is-negative')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') === -1, 'postinstall script was not executed')

  t.end()
})

test('prepublish is not executed after installation with arguments', t => {
  const project = prepare(t, {
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install', 'is-negative')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') === -1, 'prepublish script was not executed')

  t.end()
})

test('prepublish is executed after argumentless installation', t => {
  const project = prepare(t, {
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepublish script was executed')

  t.end()
})
