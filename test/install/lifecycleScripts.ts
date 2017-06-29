import tape = require('tape')
import promisifyTape from 'tape-promise'
import {install} from 'supi'
import {
  prepare,
  testDefaults,
  execPnpmSync,
} from '../utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import rimraf = require('rimraf-then')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)

test('installation fails if lifecycle script fails', t => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'exit 1'
    },
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 1, 'installation failed')

  t.end()
})

test('lifecycle script runs with the correct user agent', t => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'node --eval "console.log(process.env.npm_config_user_agent)"'
    },
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  const expectedUserAgentPrefix = `${pnpmPkg.name}/${pnpmPkg.version} `
  t.ok(result.stdout.toString().indexOf(expectedUserAgentPrefix) !== -1, 'correct npm_config_user_agent value')

  t.end()
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

test('prepare is not executed after installation with arguments', t => {
  const project = prepare(t, {
    scripts: {
      prepare: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install', 'is-negative')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') === -1, 'prepare script was not executed')

  t.end()
})

test('prepare is executed after argumentless installation', t => {
  const project = prepare(t, {
    scripts: {
      prepare: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepare script was executed')

  t.end()
})
