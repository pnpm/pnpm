import tape = require('tape')
import promisifyTape from 'tape-promise'
import {install, installPkgs} from 'supi'
import {addDistTag} from 'pnpm-registry-mock'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('prefer version ranges specified for top dependencies', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
      'pkg-with-1-dep': '*',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, when doing named installation', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
    },
  })

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(await testDefaults())
  await installPkgs(['pkg-with-1-dep'], await testDefaults())

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'])
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

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'])
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

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'])
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

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/101.0.0'])
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

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})
