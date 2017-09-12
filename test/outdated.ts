import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {stripIndents} from 'common-tags'
import {
  execPnpm,
  execPnpmSync,
  tempDir,
} from './utils'
import normalizeNewline = require('normalize-newline')

const fixture = path.join(__dirname, 'packages', 'has-outdated-deps')
const test = promisifyTape(tape)

test('pnpm outdated', async (t: tape.Test) => {
  process.chdir(fixture)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Wanted  Latest
    is-negative  1.0.0    1.1.0   2.1.0
    is-positive  1.0.0    3.1.0   3.1.0
  ` + '\n')
})
