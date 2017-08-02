import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {stripIndent} from 'common-tags'
import {
  execPnpm,
  execPnpmSync,
  tempDir,
} from './utils'

const test = promisifyTape(tape)

test('listing global packages', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', 'is-positive@3.1.0')

  const result = execPnpmSync('list', '-g')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    pnpm-global-pkg@1.0.0 ${path.join(global, 'pnpm-global', '1')}
    └── is-positive@3.1.0
  ` + '\n\n')
})

test('listing global packages installed with independent-leaves = true', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', '--independent-leaves', 'is-positive@3.1.0')

  const result = execPnpmSync('list', '-g', '--independent-leaves')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    pnpm-global-pkg@1.0.0 ${path.join(global, 'pnpm-global', '1_independent_leaves')}
    └── is-positive@3.1.0
  ` + '\n\n')
})
