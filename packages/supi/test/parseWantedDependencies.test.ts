import getSpecFromPackageManifest from 'supi/lib/getSpecFromPackageManifest'
import promisifyTape from 'tape-promise'
import tape = require('tape')

const test = promisifyTape(tape)

test('getSpecFromPackageManifest()', (t) => {
  t.equal(
    getSpecFromPackageManifest({
      dependencies: {
        foo: '3.0.0',
      },
      devDependencies: {
        foo: '2.0.0',
      },
      optionalDependencies: {
        foo: '1.0.0',
      },
    }, 'foo'),
    '1.0.0',
    'optionalDependencies is first priority'
  )
  t.equal(
    getSpecFromPackageManifest({
      dependencies: {
        foo: '3.0.0',
      },
      devDependencies: {
        foo: '2.0.0',
      },
    }, 'foo'),
    '3.0.0',
    'dependencies is second priority'
  )
  t.equal(
    getSpecFromPackageManifest({
      devDependencies: {
        foo: '2.0.0',
      },
    }, 'foo'),
    '2.0.0',
    'devDependencies is third priority'
  )
  t.end()
})
