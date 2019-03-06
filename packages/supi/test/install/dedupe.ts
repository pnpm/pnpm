import prepare from '@pnpm/prepare'
import pnpmRegistryMock = require('pnpm-registry-mock')
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const addDistTag = pnpmRegistryMock.addDistTag

test('prefer version ranges specified for top dependencies', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
      'pkg-with-1-dep': '*',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, when doing named installation', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())
  await addDependenciesToPackage(['pkg-with-1-dep'], await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if they are aliased', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'foo': 'npm:dep-of-pkg-with-1-dep@100.0.0',
      'pkg-with-1-dep': '*',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if the subdependencies are aliased', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
      'pkg-with-1-aliased-dep': '100.0.0',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('ignore version of root dependency when it is incompatible with the indirect dependency\'s range', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '101.0.0',
      'pkg-with-1-dep': '100.0.0',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  await install(await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/101.0.0'])
})

test('prefer dist-tag specified for top dependency', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': 'stable',
      'pkg-with-1-dep': '100.0.0',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'stable' })

  await install(await testDefaults())

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges passed in via opts.preferredVersions', async (t: tape.Test) => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '^100.0.0',
      'pkg-with-1-dep': '*',
    },
  })

  await install(await testDefaults({
    preferredVersions: {
      'dep-of-pkg-with-1-dep': {
        selector: '100.0.0',
        type: 'version',
      },
    },
  }))

  const lockfile = await project.loadLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})
