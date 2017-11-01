import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
  pathToLocalPkg,
  addDistTag,
} from './utils'
import writeJsonFile = require('write-json-file')
import {
  link,
  unlinkPkgs,
  unlink,
  installPkgs,
} from 'supi'
import isInnerLink = require('is-inner-link')
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')

const test = promisifyTape(tape)

test('unlink 1 package that exists in package.json', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-subdir': '^1.0.0',
      'is-positive': '^1.0.0',
    }
  })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      name: 'is-subdir',
      version: '1.0.0',
      dependencies: {
        'is-windows': '^1.0.0'
      }
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  await link('is-subdir', 'project')
  await link('is-positive', 'project')

  process.chdir('project')
  await unlinkPkgs(['is-subdir'], testDefaults())

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.notOk((await isInnerLink('node_modules', 'is-positive')).isInner, 'is-positive left linked')
})

test("don't update package when unlinking", async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('foo', '100.0.0', 'latest')
  await installPkgs(['foo'], testDefaults())

  process.chdir('..')

  await writeJsonFile('foo/package.json', {
    name: 'foo',
    version: '100.0.0',
  })

  await link('foo', 'project')
  await addDistTag('foo', '100.1.0', 'latest')

  process.chdir('project')
  await unlinkPkgs(['foo'], testDefaults())

  t.equal(project.requireModule('foo/package.json').version, '100.0.0', 'foo not updated after unlink')
})

test('unlink 2 packages. One of them exists in package.json', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-subdir': '^1.0.0',
    }
  })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      name: 'is-subdir',
      version: '1.0.0',
      dependencies: {
        'is-windows': '^1.0.0'
      }
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  await link('is-subdir', 'project')
  await link('is-positive', 'project')

  process.chdir('project')
  await unlinkPkgs(['is-subdir', 'is-positive'], testDefaults())

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.notOk(await exists(path.join('node_modules', 'is-positive')), 'is-positive removed as it is not in package.json')
})

test('unlink all packages', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-subdir': '^1.0.0',
      '@zkochan/logger': '^0.1.0',
    }
  })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      name: 'is-subdir',
      version: '1.0.0',
      dependencies: {
        'is-windows': '^1.0.0'
      }
    }),
    writeJsonFile('logger/package.json', {
      name: '@zkochan/logger',
      version: '0.1.0',
    }),
  ])

  await link('is-subdir', 'project')
  await link('logger', 'project')

  process.chdir('project')
  await unlink(testDefaults())

  t.equal(typeof project.requireModule('is-subdir'), 'function', 'is-subdir installed after unlinked')
  t.equal(typeof project.requireModule('@zkochan/logger'), 'object', '@zkochan/logger installed after unlinked')
})

test("don't warn about scoped packages when running unlink w/o params", async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['@zkochan/logger'], testDefaults())

  const reporter = sinon.spy()
  await unlink(testDefaults({reporter}))

  t.notOk(reporter.calledWithMatch({
    level: 'warn',
    message: '@zkochan/logger is not an external link',
  }), 'not reported warning')
})

test("don't unlink package that is not a link", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await installPkgs(['is-positive'], testDefaults())

  await unlinkPkgs(['is-positive'], testDefaults({reporter}))

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: 'is-positive is not an external link',
  }), 'reported warning')
})

test("don't unlink package that is not a link when independent-leaves = true", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await installPkgs(['is-positive'], testDefaults({independentLeaves: true}))

  await unlinkPkgs(['is-positive'], testDefaults({independentLeaves: true, reporter}))

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: 'is-positive is not an external link',
  }), 'reported warning')
})
