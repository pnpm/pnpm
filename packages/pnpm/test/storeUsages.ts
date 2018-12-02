import { tempDir } from '@pnpm/prepare'
import tape = require('tape');
import promisifyTape from 'tape-promise'
import { execPnpm } from './utils'

const test = promisifyTape(tape);

test('pnpm store usages CLI does not fail', async function (t: tape.Test) {
  tempDir(t);

  // Call store usages
  await execPnpm('store', 'usages', 'is-odd@2.0.0 @babel/core ansi-regex');
  t.pass('CLI did not fail')
});
