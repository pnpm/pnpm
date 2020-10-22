import createVersionOverrider from 'supi/lib/install/createVersionsOverrider'
import promisifyTape from 'tape-promise'
import tape = require('tape')

const test = promisifyTape(tape)

test('createVersionsOverrider() overrides dependencies of specified packages only', (t: tape.Test) => {
  const overrider = createVersionOverrider({
    'foo@1>bar@^1.2.0': '3.0.0',
  })
  t.deepEqual(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      bar: '^1.2.0',
    },
  }), {
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      bar: '3.0.0',
    },
  })
  t.deepEqual(overrider({
    name: 'foo',
    version: '2.0.0',
    dependencies: {
      bar: '^1.2.0',
    },
  }), {
    name: 'foo',
    version: '2.0.0',
    dependencies: {
      bar: '^1.2.0',
    },
  })
  t.end()
})
