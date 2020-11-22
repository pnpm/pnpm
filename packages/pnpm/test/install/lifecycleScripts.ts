import prepare from '@pnpm/prepare'
import { PackageManifest } from '@pnpm/types'
import promisifyTape from 'tape-promise'
import { execPnpmSync } from '../utils'
import path = require('path')
import PATH = require('path-name')
import loadJsonFile = require('load-json-file')
import tape = require('tape')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync<PackageManifest>(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)

test('installation fails if lifecycle script fails', t => {
  prepare(t, {
    scripts: {
      preinstall: 'exit 1',
    },
  })

  const result = execPnpmSync(['install'])

  t.equal(result.status, 1, 'installation failed')

  t.end()
})

test('lifecycle script runs with the correct user agent', t => {
  prepare(t, {
    scripts: {
      preinstall: 'node --eval "console.log(process.env.npm_config_user_agent)"',
    },
  })

  const result = execPnpmSync(['install'])

  t.equal(result.status, 0, 'installation was successfull')
  const expectedUserAgentPrefix = `${pnpmPkg.name}/${pnpmPkg.version} `
  t.ok(result.stdout.toString().includes(expectedUserAgentPrefix), 'correct npm_config_user_agent value')

  t.end()
})

test('preinstall is executed before general installation', t => {
  prepare(t, {
    scripts: {
      preinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().includes('Hello world!'), 'preinstall script was executed')

  t.end()
})

test('postinstall is executed after general installation', t => {
  prepare(t, {
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().includes('Hello world!'), 'postinstall script was executed')

  t.end()
})

test('postinstall is not executed after named installation', t => {
  prepare(t, {
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(!result.stdout.toString().includes('Hello world!'), 'postinstall script was not executed')

  t.end()
})

test('prepare is not executed after installation with arguments', t => {
  prepare(t, {
    scripts: {
      prepare: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(!result.stdout.toString().includes('Hello world!'), 'prepare script was not executed')

  t.end()
})

test('prepare is executed after argumentless installation', t => {
  prepare(t, {
    scripts: {
      prepare: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().includes('Hello world!'), 'prepare script was executed')

  t.end()
})

test('lifecycle events have proper npm_config_argv', async (t: tape.Test) => {
  prepare(t, {
    dependencies: {
      'write-lifecycle-env': '^1.0.0',
    },
    scripts: {
      postinstall: 'write-lifecycle-env',
    },
  })

  execPnpmSync(['install'])

  const lifecycleEnv = await loadJsonFile<object>('env.json')

  t.deepEqual(JSON.parse(lifecycleEnv['npm_config_argv']), {
    cooked: ['install'],
    original: ['install'],
    remain: ['install'],
  })
})

test('dependency should not be added to package.json and lockfile if it was not built successfully', async (t: tape.Test) => {
  const project = prepare(t, { name: 'foo', version: '1.0.0' })

  const result = execPnpmSync(['install', 'package-that-cannot-be-installed@0.0.0'])

  t.equal(result.status, 1)

  t.notOk(await project.readCurrentLockfile())
  t.notOk(await project.readLockfile())

  const pkg = await import(path.resolve('package.json'))
  t.deepEqual(pkg, { name: 'foo', version: '1.0.0' }, 'package.json not updated')
})

test('node-gyp is in the PATH', async (t) => {
  prepare(t, {
    scripts: {
      test: 'node-gyp --help',
    },
  })

  // `npm test` adds node-gyp to the PATH
  // it is removed here to test that pnpm adds it
  const initialPath = process.env.PATH

  if (typeof initialPath !== 'string') throw new Error('PATH is not defined')

  process.env[PATH] = initialPath
    .split(path.delimiter)
    .filter((p: string) => !p.includes('node-gyp-bin') && !p.includes('npm'))
    .join(path.delimiter)

  const result = execPnpmSync(['test'])

  process.env[PATH] = initialPath

  t.equal(result.status, 0)
})
