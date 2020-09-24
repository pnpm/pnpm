import { requirePnpmfile } from '@pnpm/pnpmfile'
import path = require('path')
import test = require('tape')

test('ignoring a pnpmfile that exports undefined', (t) => {
  const pnpmfile = requirePnpmfile(path.join(__dirname, 'pnpmfiles/undefined.js'), __dirname)
  t.equal(typeof pnpmfile, 'undefined')
  t.end()
})

test('readPackage hook run fails when returns undefined ', (t) => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoReturn.js')
  const pnpmfile = requirePnpmfile(pnpmfilePath, __dirname)
  try {
    pnpmfile.hooks.readPackage({})
    t.fail('readPackage hook run should fail')
  } catch (err) {
    t.equal(err.message, `readPackage hook did not return a package manifest object. Hook imported via ${pnpmfilePath}`)
    t.equal(err.code, 'ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT')
    t.end()
  }
})

test('readPackage hook run fails when returned dependencies is not an object ', (t) => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoObject.js')
  const pnpmfile = requirePnpmfile(pnpmfilePath, __dirname)
  try {
    pnpmfile.hooks.readPackage({})
    t.fail('readPackage hook run should fail')
  } catch (err) {
    t.equal(err.message, `readPackage hook returned package manifest object's property 'dependencies' must be an object. Hook imported via ${pnpmfilePath}`)
    t.equal(err.code, 'ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT')
    t.end()
  }
})
