import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import pnpmRegistryMock = require('@pnpm/registry-mock')
import tape = require('tape')

const test = promisifyTape(tape)
const addDistTag = pnpmRegistryMock.addDistTag

test('prefer version ranges specified for top dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        'pkg-with-1-dep': '*',
      },
    },
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, when doing named installation', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const manifest = await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
      },
    },
    await testDefaults()
  )
  await addDependenciesToPackage(manifest, ['pkg-with-1-dep'], await testDefaults())

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if they are aliased', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        foo: 'npm:dep-of-pkg-with-1-dep@100.0.0',
        'pkg-with-1-dep': '*',
      },
    },
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if the subdependencies are aliased', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        'pkg-with-1-aliased-dep': '100.0.0',
      },
    },
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('ignore version of root dependency when it is incompatible with the indirect dependency\'s range', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': '101.0.0',
        'pkg-with-1-dep': '100.0.0',
      },
    },
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/101.0.0'])
})

test('prefer dist-tag specified for top dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'stable' })

  await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': 'stable',
        'pkg-with-1-dep': '100.0.0',
      },
    },
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges passed in via opts.preferredVersions', async (t: tape.Test) => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const project = prepareEmpty(t)

  await install(
    {
      dependencies: {
        'dep-of-pkg-with-1-dep': '^100.0.0',
        'pkg-with-1-dep': '*',
      },
    },
    await testDefaults(
      {
        preferredVersions: {
          'dep-of-pkg-with-1-dep': {
            '100.0.0': 'version',
          },
        },
      }
    )
  )

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/1187
test('prefer version of package that also satisfies the range of the same package higher in the dependency graph', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    {},
    ['has-foo-as-dep-and-subdep'],
    await testDefaults()
  )

  const lockfile = await project.readLockfile()

  t.deepEqual(
    Object.keys(lockfile.packages),
    [
      '/foo/100.0.0',
      '/has-foo-as-dep-and-subdep/1.0.0',
      '/requires-any-foo/1.0.0',
    ]
  )
})
