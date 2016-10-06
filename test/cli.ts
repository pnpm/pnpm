import path = require('path')
import test = require('tape')
import spawn = require('cross-spawn')

const pnpmBin = path.join(__dirname, '../src/bin/pnpm.ts')

test('return error status code when underlying command fails', t => {
  const result = spawn.sync('ts-node', [pnpmBin, 'invalid-command'])

  t.equal(result.status, 1, 'error status code returned')

  t.end()
})
