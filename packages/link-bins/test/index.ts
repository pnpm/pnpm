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

test('linkBins()', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(await exists(binLocation)).toBe(true)
  const content = await fs.readFile(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = await fs.stat(binFile)
    expect(stat.mode).toBe(parseInt('100755', 8))
    expect(stat.isFile()).toBe(true)
  }
})

test('linkBins() finds exotic manifests', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(exoticManifestFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(await exists(binLocation)).toBe(true)
  const content = await fs.readFile(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = await fs.stat(binFile)
    expect(stat.mode).toBe(parseInt('100755', 8))
    expect(stat.isFile()).toBe(true)
  }
})

test('linkBins() do not fail on directory w/o manifest file', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(fixtures, 'dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: false,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() with exotic manifests do not fail on directory w/o manifest file', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(fixtures, 'dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() does not link own bins', async () => {
  const target = tempy.directory()
  await ncp(foobarFixture, target)

  const warn = jest.fn()
  const modules = path.join(target, 'node_modules')
  const binTarget = path.join(target, 'node_modules', 'foo', 'node_modules', '.bin')

  await linkBins(modules, binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['bar']))
})

test('linkBinsOfPackages()', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

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

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(await exists(binLocation)).toBe(true)
  const content = await fs.readFile(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')
})

test('linkBins() resolves conflicts. Prefer packages that use their name as bin name', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  expect(warn).toHaveBeenCalledWith(`Cannot link binary 'bar' of 'foo' to '${binTarget}': binary of 'bar' is already linked`, 'BINARIES_CONFLICT')
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(await exists(binLocation)).toBe(true)
    const content = await fs.readFile(binLocation, 'utf8')
    expect(content).toMatch('node_modules/bar/index.js')
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    expect(await exists(binLocation)).toBe(true)
    const content = await fs.readFile(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBinsOfPackages() resolves conflicts. Prefer packages that use their name as bin name', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

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

  expect(warn).toHaveBeenCalledWith(`Cannot link binary 'bar' of 'foo' to '${binTarget}': binary of 'bar' is already linked`, 'BINARIES_CONFLICT')
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(await exists(binLocation)).toBe(true)
    const content = await fs.readFile(binLocation, 'utf8')
    expect(content).toMatch('node_modules/bar/index.js')
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    expect(await exists(binLocation)).toBe(true)
    const content = await fs.readFile(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBins() would throw error if package has no name field', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  try {
    await linkBins(path.join(noNameFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn,
    })
    fail('linkBins should fail when package has no name')
  } catch (err) {
    const packagePath = normalizePath(path.join(noNameFixture, 'node_modules/simple'))
    expect(err.message).toEqual(`Package in ${packagePath} must have a name to get bin linked.`)
    expect(err.code).toEqual('ERR_PNPM_INVALID_PACKAGE_NAME')
    expect(warn).not.toHaveBeenCalled()
  }
})

test('linkBins() would give warning if package has no bin field', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(noBinFixture, 'packages'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  const packagePath = normalizePath(path.join(noBinFixture, 'packages/simple'))
  expect(warn).toHaveBeenCalledWith(`Package in ${packagePath} must have a non-empty bin field to get bin linked.`, 'EMPTY_BIN')
})

test('linkBins() would not give warning if package has no bin field but inside node_modules', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(path.join(noBinFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})
