import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  addDistTag,
  testDefaults,
} from '../utils'
import {installPkgs, PackageManifest} from 'supi'

const test = promisifyTape(tape)

test('readPackage hook', async (t: tape.Test) => {
  const project = prepare(t)

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  function readPackageHook (pkg: PackageManifest) {
    if (pkg.name === 'pkg-with-1-dep') {
      pkg!.dependencies!['dep-of-pkg-with-1-dep'] = '100.0.0'
    }
    return pkg
  }

  await installPkgs(['pkg-with-1-dep'], testDefaults({
    hooks: {readPackage: readPackageHook}
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})
