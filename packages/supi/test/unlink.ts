import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from './utils'
import fs = require('fs')
import isInnerLink = require('is-inner-link')
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')
import tape = require('tape')
import writeJsonFile = require('write-json-file')

const test = promisifyTape(tape)

test('unlink 1 package that exists in package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  const opts = await testDefaults({ fastUnpack: false, store: path.resolve('.store') })

  let manifest = await link(
    ['is-subdir', 'is-positive'],
    path.join('project', 'node_modules'),
    {
      ...opts,
      dir: path.resolve('project'),
      manifest: {
        dependencies: {
          'is-positive': '^1.0.0',
          'is-subdir': '^1.0.0',
        },
      },
    }
  )

  process.chdir('project')

  manifest = await install(manifest, opts)

  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.notOk((await isInnerLink('node_modules', 'is-positive')).isInner, 'is-positive left linked')
})

test("don't update package when unlinking", async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('foo', '100.0.0', 'latest')
  const opts = await testDefaults({ dir: process.cwd() })
  let manifest = await addDependenciesToPackage({}, ['foo'], opts)

  process.chdir('..')

  await writeJsonFile('foo/package.json', {
    name: 'foo',
    version: '100.0.0',
  })

  manifest = await link(['foo'], path.join('project', 'node_modules'), { ...opts, manifest })
  await addDistTag('foo', '100.1.0', 'latest')

  process.chdir('project')
  await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  t.equal(project.requireModule('foo/package.json').version, '100.0.0', 'foo not updated after unlink')
})

test(`don't update package when unlinking. Initial link is done on a package w/o ${WANTED_LOCKFILE}`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults({ dir: process.cwd() })
  process.chdir('..')

  await writeJsonFile('foo/package.json', {
    name: 'foo',
    version: '100.0.0',
  })

  const manifest = await link(['foo'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        foo: '^100.0.0',
      },
    },
  })
  await addDistTag('foo', '100.1.0', 'latest')

  process.chdir('project')
  const unlinkResult = await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  t.equal(project.requireModule('foo/package.json').version, '100.1.0', 'latest foo is installed')
  t.deepEqual(unlinkResult[0].manifest.dependencies, { foo: '^100.0.0' }, 'package.json not updated')
})

test('unlink 2 packages. One of them exists in package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults({ fastUnpack: false, dir: process.cwd() })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  const manifest = await link(['is-subdir', 'is-positive'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        'is-subdir': '^1.0.0',
      },
    },
  })

  process.chdir('project')
  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir', 'is-positive'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.notOk(await exists(path.join('node_modules', 'is-positive')), 'is-positive removed as it is not in package.json')
})

test('unlink all packages', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults({ fastUnpack: false, dir: process.cwd() })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('logger/package.json', {
      name: '@zkochan/logger',
      version: '0.1.0',
    }),
  ])

  const manifest = await link(['is-subdir', 'logger'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        '@zkochan/logger': '^0.1.0',
        'is-subdir': '^1.0.0',
      },
    },
  })

  await mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        rootDir: path.resolve('project'),
      },
    ],
    opts
  )

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.equal(typeof project.requireModule('@zkochan/logger'), 'object', '@zkochan/logger installed after unlinked')
})

test("don't warn about scoped packages when running unlink w/o params", async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['@zkochan/logger'], await testDefaults())

  const reporter = sinon.spy()
  await mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ reporter })
  )

  t.notOk(reporter.calledWithMatch({
    level: 'warn',
    message: '@zkochan/logger is not an external link',
  }), 'not reported warning')
})

test("don't unlink package that is not a link", async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults())

  await mutateModules(
    [
      {
        dependencyNames: ['is-positive'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ reporter })
  )

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: 'is-positive is not an external link',
  }), 'reported warning')
})

test('unlink would remove global bin', async (t: tape.Test) => {
  prepareEmpty(t)
  process.chdir('..')
  fs.mkdirSync('bin')
  fs.mkdirSync('is-subdir')
  fs.writeFileSync('is-subdir/index.js', ' ')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      bin: 'index.js',
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
  ])

  const opts = await testDefaults({
    fastUnpack: false,
    globalBin: path.resolve('bin'),
    linkToBin: path.resolve('bin'),
    store: path.resolve('.store'),
  })

  const manifest = await link(
    ['is-subdir'],
    path.join('project', 'node_modules'),
    {
      ...opts,
      dir: path.resolve('project'),
      manifest: {
        dependencies: {
          'is-subdir': '^1.0.0',
        },
        name: 'is-subdir',
      },
    }
  )
  t.ok(fs.existsSync(path.resolve('bin/is-subdir')), 'bin is installed in global bin directory')

  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  t.notOk(fs.existsSync(path.resolve('bin/is-subdir')), 'bin is removed in global bin directory')
})
