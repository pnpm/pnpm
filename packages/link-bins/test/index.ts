/// <reference path="../../../typings/index.d.ts"/>
import { promisify } from 'util'
import linkBins, {
  linkBinsOfPackages,
} from '@pnpm/link-bins'
import path = require('path')
import isWindows = require('is-windows')
import fs = require('mz/fs')
import ncpcb = require('ncp')
import normalizePath = require('normalize-path')
import exists = require('path-exists')
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')

const ncp = promisify(ncpcb)

// The fixtures directory is copied to fixtures_for_testing before the tests run
// This happens because the tests conver some of the files into executables
const fixtures = path.join(__dirname, 'fixtures_for_testing')
const simpleFixture = path.join(fixtures, 'simple-fixture')
const binNameConflictsFixture = path.join(fixtures, 'bin-name-conflicts')
const foobarFixture = path.join(fixtures, 'foobar')
const exoticManifestFixture = path.join(fixtures, 'exotic-manifest')
const noNameFixture = path.join(fixtures, 'no-name')
const noBinFixture = path.join(fixtures, 'no-bin')

const POWER_SHELL_IS_SUPPORTED = isWindows()
const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS

function getExpectedBins (bins: string[]) {
  const expectedBins = [...bins]
  if (POWER_SHELL_IS_SUPPORTED) {
    bins.forEach((bin) => expectedBins.push(`${bin}.ps1`))
  }
  if (IS_WINDOWS) {
    bins.forEach((bin) => expectedBins.push(`${bin}.cmd`))
  }
  return expectedBins.sort()
}

test('linkBins()', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  t.notOk(warn.called)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  t.ok(await exists(binLocation))
  const content = await fs.readFile(binLocation, 'utf8')
  t.ok(content.includes('node_modules/simple/index.js'))

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = await fs.stat(binFile)
    t.equal(stat.mode, parseInt('100755', 8), `${binFile} is executable`)
    t.ok(stat.isFile(), `${binFile} refers to a file`)
  }

  t.end()
})

test('linkBins() finds exotic manifests', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(exoticManifestFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  t.notOk(warn.called)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  t.ok(await exists(binLocation))
  const content = await fs.readFile(binLocation, 'utf8')
  t.ok(content.includes('node_modules/simple/index.js'))

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = await fs.stat(binFile)
    t.equal(stat.mode, parseInt('100755', 8), `${binFile} is executable`)
    t.ok(stat.isFile(), `${binFile} refers to a file`)
  }

  t.end()
})

test('linkBins() do not fail on directory w/o manifest file', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(fixtures, 'dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: false,
    warn,
  })

  t.notOk(warn.called)
  t.end()
})

test('linkBins() with exotic manifests do not fail on directory w/o manifest file', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(fixtures, 'dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  t.notOk(warn.called)
  t.end()
})

test('linkBins() does not link own bins', async (t) => {
  const target = tempy.directory()
  await ncp(foobarFixture, target)

  const warn = sinon.spy()
  const modules = path.join(target, 'node_modules')
  const binTarget = path.join(target, 'node_modules', 'foo', 'node_modules', '.bin')

  await linkBins(modules, binTarget, { warn })

  t.notOk(warn.called)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['bar']))

  t.end()
})

test('linkBinsOfPackages()', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBinsOfPackages(
    [
      {
        location: path.join(simpleFixture, 'node_modules/simple'),
        manifest: await import(path.join(simpleFixture, 'node_modules/simple/package.json')),
      },
    ],
    binTarget,
    { warn }
  )

  t.notOk(warn.called)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  t.ok(await exists(binLocation))
  const content = await fs.readFile(binLocation, 'utf8')
  t.ok(content.includes('node_modules/simple/index.js'))
  t.end()
})

test('linkBins() resolves conflicts. Prefer packages that use their name as bin name', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  t.equal(warn.args[0][0], `Cannot link binary 'bar' of 'foo' to '${binTarget}': binary of 'bar' is already linked`)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    t.ok(await exists(binLocation))
    const content = await fs.readFile(binLocation, 'utf8')
    t.ok(content.includes('node_modules/bar/index.js'))
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    t.ok(await exists(binLocation))
    const content = await fs.readFile(binLocation, 'utf8')
    t.ok(content.includes('node_modules/foo/index.js'))
  }

  t.end()
})

test('linkBinsOfPackages() resolves conflicts. Prefer packages that use their name as bin name', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  const modulesPath = path.join(binNameConflictsFixture, 'node_modules')

  await linkBinsOfPackages(
    [
      {
        location: path.join(modulesPath, 'bar'),
        manifest: await import(path.join(modulesPath, 'bar', 'package.json')),
      },
      {
        location: path.join(modulesPath, 'foo'),
        manifest: await import(path.join(modulesPath, 'foo', 'package.json')),
      },
    ],
    binTarget,
    { warn }
  )

  t.equal(warn.args[0][0], `Cannot link binary 'bar' of 'foo' to '${binTarget}': binary of 'bar' is already linked`)
  t.deepEqual(await fs.readdir(binTarget), getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    t.ok(await exists(binLocation))
    const content = await fs.readFile(binLocation, 'utf8')
    t.ok(content.includes('node_modules/bar/index.js'))
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    t.ok(await exists(binLocation))
    const content = await fs.readFile(binLocation, 'utf8')
    t.ok(content.includes('node_modules/foo/index.js'))
  }

  t.end()
})

test('linkBins() would throw error if package has no name field', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  try {
    await linkBins(path.join(noNameFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn,
    })
    t.fail('linkBins should fail when package has no name')
  } catch (err) {
    const packagePath = normalizePath(path.join(noNameFixture, 'node_modules/simple'))
    t.equal(err.message, `Package in ${packagePath} must have a name to get bin linked.`)
    t.equal(err.code, 'ERR_PNPM_INVALID_PACKAGE_NAME')
    t.notOk(warn.called)
    t.end()
  }
})

test('linkBins() would give warning if package has no bin field', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(noBinFixture, 'packages'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  const packagePath = normalizePath(path.join(noBinFixture, 'packages/simple'))
  t.ok(warn.calledWith(`Package in ${packagePath} must have a non-empty bin field to get bin linked.`))
  t.end()
})

test('linkBins() would not give warning if package has no bin field but inside node_modules', async (t) => {
  const binTarget = tempy.directory()
  t.comment(`linking bins to ${binTarget}`)
  const warn = sinon.spy()

  await linkBins(path.join(noBinFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  t.notOk(warn.called)
  t.end()
})
