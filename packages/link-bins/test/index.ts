/// <reference path="../../../typings/index.d.ts"/>
import { promises as fs, writeFileSync } from 'fs'
import path from 'path'
import { logger, globalWarn } from '@pnpm/logger'
import {
  linkBins,
  linkBinsOfPackages,
} from '@pnpm/link-bins'
import fixtures from '@pnpm/test-fixtures'
import CMD_EXTENSION from 'cmd-extension'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import exists from 'path-exists'
import tempy from 'tempy'

jest.mock('@pnpm/logger', () => {
  const debug = jest.fn()
  const globalWarn = jest.fn()

  return {
    logger: () => ({ debug }),
    globalWarn,
  }
})

const binsConflictLogger = logger('bins-conflict')
// The fixture directories are copied to before the tests run
// This happens because the tests convert some of the files into executables
const f = fixtures(__dirname)

beforeEach(() => {
  binsConflictLogger.debug['mockClear']()
})

const POWER_SHELL_IS_SUPPORTED = isWindows()
const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS

const testOnWindows = IS_WINDOWS ? test : test.skip

function getExpectedBins (bins: string[]) {
  const expectedBins = [...bins]
  if (POWER_SHELL_IS_SUPPORTED) {
    bins.forEach((bin) => expectedBins.push(`${bin}.ps1`))
  }
  if (IS_WINDOWS) {
    bins.forEach((bin) => expectedBins.push(`${bin}${CMD_EXTENSION}`))
  }
  return expectedBins.sort()
}

test('linkBins()', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

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

test('linkBins() never creates a PowerShell shim for the pnpm CLI', async () => {
  const binTarget = tempy.directory()
  const fixture = f.prepare('pnpm-cli')
  const warn = jest.fn()

  await linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })

  const bins = await fs.readdir(binTarget)
  expect(bins).toContain('pnpm')
  expect(bins).not.toContain('pnpm.ps1')
})

test('linkBins() finds exotic manifests', async () => {
  const binTarget = tempy.directory()
  const exoticManifestFixture = f.prepare('exotic-manifest')
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

  await linkBins(f.find('dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: false,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() with exotic manifests do not fail on directory w/o manifest file', async () => {
  const binTarget = tempy.directory()
  const warn = jest.fn()

  await linkBins(f.find('dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() does not link own bins', async () => {
  const target = f.prepare('foobar')

  const warn = jest.fn()
  const modules = path.join(target, 'node_modules')
  const binTarget = path.join(target, 'node_modules', 'foo', 'node_modules', '.bin')

  await linkBins(modules, binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['bar']))
})

test('linkBinsOfPackages()', async () => {
  const binTarget = tempy.directory()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBinsOfPackages(
    [
      {
        location: path.join(simpleFixture, 'node_modules/simple'),
        manifest: (await import(path.join(simpleFixture, 'node_modules/simple/package.json'))).default,
      },
    ],
    binTarget
  )

  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(await exists(binLocation)).toBe(true)
  const content = await fs.readFile(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')
})

test('linkBins() resolves conflicts. Prefer packages that use their name as bin name', async () => {
  const binTarget = tempy.directory()
  const binNameConflictsFixture = f.prepare('bin-name-conflicts')
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'bar',
    binsDir: binTarget,
    linkedPkgName: 'bar',
    skippedPkgName: 'foo',
  })
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
  const binNameConflictsFixture = f.prepare('bin-name-conflicts')

  const modulesPath = path.join(binNameConflictsFixture, 'node_modules')

  await linkBinsOfPackages(
    [
      {
        location: path.join(modulesPath, 'bar'),
        manifest: (await import(path.join(modulesPath, 'bar', 'package.json'))).default,
      },
      {
        location: path.join(modulesPath, 'foo'),
        manifest: (await import(path.join(modulesPath, 'foo', 'package.json'))).default,
      },
    ],
    binTarget
  )

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'bar',
    binsDir: binTarget,
    linkedPkgName: 'bar',
    skippedPkgName: 'foo',
  })
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

test('linkBins() resolves conflicts. Prefer packages are direct dependencies', async () => {
  const binTarget = tempy.directory()
  const binNameConflictsFixture = f.prepare('bin-name-conflicts')
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, {
    projectManifest: {
      dependencies: {
        foo: '1.0.0',
      },
    },
    warn,
  })

  expect(warn).not.toHaveBeenCalled() // With(`Cannot link binary 'bar' of 'foo' to '${binTarget}': binary of 'bar' is already linked`, 'BINARIES_CONFLICT')
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(await exists(binLocation)).toBe(true)
    const content = await fs.readFile(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
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
  const noNameFixture = f.prepare('no-name')
  const warn = jest.fn()

  try {
    await linkBins(path.join(noNameFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn,
    })
    fail('linkBins should fail when package has no name')
  } catch (err: any) { // eslint-disable-line
    const packagePath = normalizePath(path.join(noNameFixture, 'node_modules/simple'))
    expect(err.message).toEqual(`Package in ${packagePath} must have a name to get bin linked.`)
    expect(err.code).toEqual('ERR_PNPM_INVALID_PACKAGE_NAME')
    expect(warn).not.toHaveBeenCalled()
  }
})

test('linkBins() would give warning if package has no bin field', async () => {
  const binTarget = tempy.directory()
  const noBinFixture = f.prepare('no-bin')
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
  const noBinFixture = f.prepare('no-bin')
  const warn = jest.fn()

  await linkBins(path.join(noBinFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() links commands from bin directory with a subdirectory', async () => {
  const binTarget = tempy.directory()

  await linkBins(f.find('bin-dir'), binTarget, { warn: () => {} })

  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['index.js']))
})

test('linkBins() fix window shebang line', async () => {
  const binTarget = tempy.directory()
  const windowShebangFixture = f.prepare('bin-window-shebang')
  const warn = jest.fn()

  await linkBins(path.join(windowShebangFixture, 'node_modules'), binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['crlf', 'lf']))

  const lfBinLoc = path.join(binTarget, 'lf')
  const crlfBinLoc = path.join(binTarget, 'crlf')
  for (const binLocation of [lfBinLoc, crlfBinLoc]) {
    expect(await exists(binLocation)).toBe(true)
  }

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const lfFilePath = path.join(windowShebangFixture, 'node_modules', 'crlf/bin/lf.js')
    const crlfFilePath = path.join(windowShebangFixture, 'node_modules', 'crlf/bin/crlf.js')

    for (const filePath of [lfFilePath, crlfFilePath]) {
      const content = await fs.readFile(filePath, 'utf8')
      expect(content.startsWith('#!/usr/bin/env node\n')).toBeTruthy()
    }

    const lfStat = await fs.stat(lfBinLoc)
    const crlfStat = await fs.stat(crlfBinLoc)
    for (const stat of [lfStat, crlfStat]) {
      expect(stat.mode).toBe(parseInt('100755', 8))
      expect(stat.isFile()).toBe(true)
    }
  }
})

test("linkBins() emits global warning when bin points to path that doesn't exist", async () => {
  const binTarget = tempy.directory()
  const binNotExistFixture = f.prepare('bin-not-exist')

  await linkBins(path.join(binNotExistFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn: () => {},
  })

  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins([]))
  expect(
    globalWarn
  ).toHaveBeenCalled()
})

testOnWindows('linkBins() shoud remove an existing .exe file from the target directory', async () => {
  const binTarget = tempy.directory()
  writeFileSync(path.join(binTarget, 'simple.exe'), '', 'utf8')
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(await fs.readdir(binTarget)).toEqual(getExpectedBins(['simple']))
})
