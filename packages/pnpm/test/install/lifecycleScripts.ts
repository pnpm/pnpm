import prepare from '@pnpm/prepare'
import { PackageJson } from '@pnpm/types'
import loadJsonFile, { sync as loadJsonFileSync } from 'load-json-file'
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpmSync } from '../utils'

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFileSync<PackageJson>(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

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
  t.ok(result.stdout.toString().includes(expectedUserAgentPrefix), 'correct npm_config_user_agent value')

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
  t.ok(result.stdout.toString().includes('Hello world!'), 'preinstall script was executed')

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
  t.ok(result.stdout.toString().includes('Hello world!'), 'postinstall script was executed')

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
  t.ok(!result.stdout.toString().includes('Hello world!'), 'postinstall script was not executed')

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
  t.ok(!result.stdout.toString().includes('Hello world!'), 'prepare script was not executed')

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
  t.ok(result.stdout.toString().includes('Hello world!'), 'prepare script was executed')

  t.end()
})

test('lifecycle events have proper npm_config_argv', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'write-lifecycle-env': '^1.0.0',
    },
    scripts: {
      postinstall: 'write-lifecycle-env',
    },
  })

  execPnpmSync('install')

  const lifecycleEnv = await loadJsonFile<object>('env.json')

  t.deepEqual(JSON.parse(lifecycleEnv['npm_config_argv']), {
    cooked: ['install'],
    original: ['install'],
    remain: ['install'],
  })
})

test('dependency should not be added to package.json and lockfile if it was not built successfully', async (t: tape.Test) => {
  const project = prepare(t, { name: 'foo', version: '1.0.0' })

  const result = execPnpmSync('install', 'package-that-cannot-be-installed@0.0.0')

  t.equal(result.status, 1)

  t.notOk(await project.loadCurrentLockfile())
  t.notOk(await project.loadLockfile())

  const pkg = await import(path.resolve('package.json'))
  t.deepEqual(pkg, { name: 'foo', version: '1.0.0' }, 'package.json not updated')
})
