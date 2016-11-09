import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import semver = require('semver')
import fs = require('mz/fs')
import prepare from './support/prepare'
import testDefaults from './support/testDefaults'
import {installPkgs} from '../lib'
import runCmd from '../lib/cmd/run'

const preserveSymlinksEnvVariable = semver.satisfies(process.version, '>=7.1.0')

test('run node in scripts with preserve symlinks mode', async function (t) {
  if (!preserveSymlinksEnvVariable) {
    t.skip('this test is only for Node.js >= 7.1.0')
    return
  }

  prepare({
    scripts: {
      test: 'node index'
    }
  })

  await fs.writeFile('index.js', `
    const fs = require('fs')
    const symlinksPreserved = require('symlinks-preserved')
    fs.writeFileSync('test-result', symlinksPreserved, 'utf8')
  `, 'utf8')

  await installPkgs(['symlinks-preserved'], testDefaults())
  const result = runCmd(['test'], {})
  t.equal(result.status, 0, 'executable exited with success')
  t.equal(await fs.readFile('test-result', 'utf8'), 'true', 'symlinks are preserved')
})
