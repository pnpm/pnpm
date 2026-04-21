/// <reference path="../../../__typings__/index.d.ts"/>
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import { cmdExtension as CMD_EXTENSION } from 'cmd-extension'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import { temporaryDirectory } from 'tempy'

jest.unstable_mockModule('@pnpm/logger', () => {
  const debug = jest.fn()
  const globalWarn = jest.fn()

  return {
    logger: () => ({ debug }),
    globalWarn,
  }
})

const { logger, globalWarn } = await import('@pnpm/logger')
const {
  linkBins,
  linkBinsOfPackages,
  linkBinsOfPkgsByAliases,
} = await import('@pnpm/bins.linker')

const binsConflictLogger = logger('bins-conflict')
// The fixture directories are copied to before the tests run
// This happens because the tests convert some of the files into executables
const f = fixtures(import.meta.dirname)

beforeEach(() => {
  jest.mocked(binsConflictLogger.debug).mockClear()
  jest.mocked(globalWarn).mockClear()
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
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(fs.existsSync(binLocation)).toBe(true)
  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = fs.statSync(binFile)
    expect(stat.mode).toBe(parseInt('100755', 8))
    expect(stat.isFile()).toBe(true)
  }
})

test('linkBins() skips bins that already reference the correct target', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  const binLocation = path.join(binTarget, 'simple')
  expect(fs.existsSync(binLocation)).toBe(true)
  const originalContent = fs.readFileSync(binLocation, 'utf8')
  // The bin contains a cmd-shim-target marker with the correct target path
  const expectedTarget = normalizePath(path.join(simpleFixture, 'node_modules', 'simple', 'index.js'))
  expect(originalContent).toContain(`# cmd-shim-target=${expectedTarget}\n`)
  // Append a sentinel to the existing (correct) content to prove it is not rewritten
  const sentinel = originalContent + '\n# sentinel'
  fs.writeFileSync(binLocation, sentinel, 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(fs.readFileSync(binLocation, 'utf8')).toBe(sentinel)
})

test('linkBins() rewrites bins that lack a target marker', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

  // Create a stale bin without a cmd-shim-target marker
  fs.mkdirSync(binTarget, { recursive: true })
  const binLocation = path.join(binTarget, 'simple')
  fs.writeFileSync(binLocation, '#!/bin/sh\n"$basedir/../wrong-pkg/index.js" "$@"', 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).not.toContain('wrong-pkg')
})

test('linkBins() never creates a PowerShell shim for the pnpm CLI', async () => {
  const binTarget = temporaryDirectory()
  const fixture = f.prepare('pnpm-cli')
  const warn = jest.fn()

  await linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })

  const bins = fs.readdirSync(binTarget)
  expect(bins).toContain('pnpm')
  expect(bins).not.toContain('pnpm.ps1')
})

test('linkBins() finds exotic manifests', async () => {
  const binTarget = temporaryDirectory()
  const exoticManifestFixture = f.prepare('exotic-manifest')
  const warn = jest.fn()

  await linkBins(path.join(exoticManifestFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(fs.existsSync(binLocation)).toBe(true)
  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const binFile = path.join(binTarget, 'simple')
    const stat = fs.statSync(binFile)
    expect(stat.mode).toBe(parseInt('100755', 8))
    expect(stat.isFile()).toBe(true)
  }
})

test('linkBins() do not fail on directory w/o manifest file', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()

  await linkBins(f.find('dir-with-no-manifest/node_modules'), binTarget, {
    allowExoticManifests: false,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() with exotic manifests do not fail on directory w/o manifest file', async () => {
  const binTarget = temporaryDirectory()
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
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['bar']))
})

test('linkBinsOfPackages()', async () => {
  const binTarget = temporaryDirectory()
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

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(fs.existsSync(binLocation)).toBe(true)
  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')
})

test('linkBinsOfPkgsByAliases()', async () => {
  const binTarget = temporaryDirectory()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBinsOfPkgsByAliases(
    [],
    binTarget,
    {
      modulesDir: path.join(simpleFixture, 'node_modules'),
      warn: () => {},
    }
  )
  expect(fs.readdirSync(binTarget)).toEqual([])

  await linkBinsOfPkgsByAliases(
    ['simple'],
    binTarget,
    {
      modulesDir: path.join(simpleFixture, 'node_modules'),
      warn: () => {},
    }
  )

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
  const binLocation = path.join(binTarget, 'simple')
  expect(fs.existsSync(binLocation)).toBe(true)
  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toMatch('node_modules/simple/index.js')
})

test('linkBins() resolves conflicts. Prefer packages that use their name as bin name', async () => {
  const binTarget = temporaryDirectory()
  const binNameConflictsFixture = f.prepare('bin-name-conflicts')
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'bar',
    binsDir: binTarget,
    linkedPkgName: 'bar',
    linkedPkgVersion: expect.any(String),
    skippedPkgName: 'foo',
    skippedPkgVersion: expect.any(String),
  })
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/bar/index.js')
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBins() resolves conflicts. Prefer packages whose name is greater in localeCompare', async () => {
  const binTarget = temporaryDirectory()
  const binNameConflictsFixture = f.prepare('bin-name-conflicts-no-own-name')
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'my-command',
    binsDir: binTarget,
    linkedPkgName: 'foo',
    linkedPkgVersion: expect.any(String),
    skippedPkgName: 'bar',
    skippedPkgVersion: expect.any(String),
  })
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['my-command']))

  {
    const binLocation = path.join(binTarget, 'my-command')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBins() resolves conflicts. Prefer the latest version of the same package', async () => {
  const binTarget = temporaryDirectory()
  const binNameConflictsFixture = f.prepare('different-versions')
  const warn = jest.fn()

  await linkBins(path.join(binNameConflictsFixture, 'node_modules'), binTarget, { warn })

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'my-command',
    binsDir: binTarget,
    linkedPkgName: 'my-command',
    linkedPkgVersion: expect.any(String),
    skippedPkgName: 'my-command',
    skippedPkgVersion: '1.0.0',
  })
  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'my-command',
    binsDir: binTarget,
    linkedPkgName: 'my-command',
    linkedPkgVersion: expect.any(String),
    skippedPkgName: 'my-command',
    skippedPkgVersion: '1.1.0',
  })
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['my-command']))

  {
    const binLocation = path.join(binTarget, 'my-command')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/my-command-greater/index.js')
  }
})

test('linkBinsOfPackages() resolves conflicts. Prefer packages that use their name as bin name', async () => {
  const binTarget = temporaryDirectory()
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
    linkedPkgAlias: undefined,
    linkedPkgName: 'bar',
    linkedPkgVersion: expect.any(String),
    skippedPkgAlias: undefined,
    skippedPkgName: 'foo',
    skippedPkgVersion: expect.any(String),
  })
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/bar/index.js')
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBinsOfPackages() resolves conflicts. Prefer the latest version', async () => {
  const binTarget = temporaryDirectory()
  const binNameConflictsFixture = f.prepare('different-versions')

  const modulesPath = path.join(binNameConflictsFixture, 'node_modules')

  await linkBinsOfPackages(
    [
      {
        location: path.join(modulesPath, 'my-command-lesser'),
        manifest: (await import(path.join(modulesPath, 'my-command-lesser', 'package.json'))).default,
      },
      {
        location: path.join(modulesPath, 'my-command-middle'),
        manifest: (await import(path.join(modulesPath, 'my-command-middle', 'package.json'))).default,
      },
      {
        location: path.join(modulesPath, 'my-command-greater'),
        manifest: (await import(path.join(modulesPath, 'my-command-greater', 'package.json'))).default,
      },
    ],
    binTarget
  )

  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'my-command',
    binsDir: binTarget,
    linkedPkgAlias: undefined,
    linkedPkgName: 'my-command',
    linkedPkgVersion: expect.any(String),
    skippedPkgAlias: undefined,
    skippedPkgName: 'my-command',
    skippedPkgVersion: '1.0.0',
  })
  expect(binsConflictLogger.debug).toHaveBeenCalledWith({
    binaryName: 'my-command',
    binsDir: binTarget,
    linkedPkgAlias: undefined,
    linkedPkgName: 'my-command',
    linkedPkgVersion: expect.any(String),
    skippedPkgAlias: undefined,
    skippedPkgName: 'my-command',
    skippedPkgVersion: '1.1.0',
  })
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['my-command']))

  {
    const binLocation = path.join(binTarget, 'my-command')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/my-command-greater/index.js')
  }
})

test('linkBins() resolves conflicts. Prefer packages are direct dependencies', async () => {
  const binTarget = temporaryDirectory()
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
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['bar', 'foofoo']))

  {
    const binLocation = path.join(binTarget, 'bar')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }

  {
    const binLocation = path.join(binTarget, 'foofoo')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    expect(content).toMatch('node_modules/foo/index.js')
  }
})

test('linkBins() would throw error if package has no name field', async () => {
  const binTarget = temporaryDirectory()
  const noNameFixture = f.prepare('no-name')
  const warn = jest.fn()
  const packagePath = normalizePath(path.join(noNameFixture, 'node_modules/simple'))

  await expect(
    linkBins(path.join(noNameFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn,
    })
  ).rejects.toMatchObject({
    message: `Package in ${packagePath} must have a name to get bin linked.`,
    code: 'ERR_PNPM_INVALID_PACKAGE_NAME',
  })
  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() would give warning if package has no bin field', async () => {
  const binTarget = temporaryDirectory()
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
  const binTarget = temporaryDirectory()
  const noBinFixture = f.prepare('no-bin')
  const warn = jest.fn()

  await linkBins(path.join(noBinFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn,
  })

  expect(warn).not.toHaveBeenCalled()
})

test('linkBins() links commands from bin directory with a subdirectory', async () => {
  const binTarget = temporaryDirectory()

  await linkBins(f.find('bin-dir'), binTarget, { warn: () => {} })

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['index.js']))
})

test('linkBins() fix window shebang line', async () => {
  const binTarget = temporaryDirectory()
  const windowShebangFixture = f.prepare('bin-window-shebang')
  const warn = jest.fn()

  await linkBins(path.join(windowShebangFixture, 'node_modules'), binTarget, { warn })

  expect(warn).not.toHaveBeenCalled()
  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['crlf', 'lf']))

  const lfBinLoc = path.join(binTarget, 'lf')
  const crlfBinLoc = path.join(binTarget, 'crlf')
  for (const binLocation of [lfBinLoc, crlfBinLoc]) {
    expect(fs.existsSync(binLocation)).toBe(true)
  }

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    const lfFilePath = path.join(windowShebangFixture, 'node_modules', 'crlf/bin/lf.js')
    const crlfFilePath = path.join(windowShebangFixture, 'node_modules', 'crlf/bin/crlf.js')

    for (const filePath of [lfFilePath, crlfFilePath]) {
      const content = fs.readFileSync(filePath, 'utf8')
      expect(content.startsWith('#!/usr/bin/env node\n')).toBeTruthy()
    }

    const lfStat = fs.statSync(lfBinLoc)
    const crlfStat = fs.statSync(crlfBinLoc)
    for (const stat of [lfStat, crlfStat]) {
      expect(stat.mode).toBe(parseInt('100755', 8))
      expect(stat.isFile()).toBe(true)
    }
  }
})

test("linkBins() emits global warning when bin points to path that doesn't exist", async () => {
  const binTarget = temporaryDirectory()
  const binNotExistFixture = f.prepare('bin-not-exist')

  await linkBins(path.join(binNotExistFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn: () => {},
  })

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins([]))
  expect(
    globalWarn
  ).toHaveBeenCalled()
})

testOnWindows('linkBins() should remove an existing .exe file from the target directory', async () => {
  const binTarget = temporaryDirectory()
  fs.writeFileSync(path.join(binTarget, 'simple.exe'), '', 'utf8')
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
})

test('linkBins() should handle bin field pointing to a directory gracefully', async () => {
  const binTarget = temporaryDirectory()
  const binIsDirFixture = f.prepare('bin-is-directory')
  const warn = jest.fn()

  await linkBins(path.join(binIsDirFixture, 'node_modules'), binTarget, { warn })

  expect(fs.readdirSync(binTarget)).toEqual([])
  expect(globalWarn).toHaveBeenCalled()
})

describe('enable prefer-symlinked-executables', () => {
  test('linkBins()', async () => {
    const binTarget = temporaryDirectory()
    const warn = jest.fn()
    const simpleFixture = f.prepare('simple-fixture')

    await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn, preferSymlinkedExecutables: true })

    expect(warn).not.toHaveBeenCalled()
    expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['simple']))
    const binLocation = path.join(binTarget, 'simple')
    expect(fs.existsSync(binLocation)).toBe(true)
    const content = fs.readFileSync(binLocation, 'utf8')
    if (IS_WINDOWS) {
      expect(content).toMatch('node_modules/simple/index.js')
    } else {
      expect(content).toMatch('console.log(\'hello_world\')')
    }

    if (EXECUTABLE_SHEBANG_SUPPORTED) {
      const binFile = path.join(binTarget, 'simple')
      const stat = fs.statSync(binFile)
      expect(stat.mode).toBe(parseInt('100755', 8))
      expect(stat.isFile()).toBe(true)
      const stdout = spawnSync(binFile).stdout.toString('utf-8')
      expect(stdout).toMatch('hello_world')
    }
  })

  test("linkBins() emits global warning when bin points to path that doesn't exist", async () => {
    const binTarget = temporaryDirectory()
    const binNotExistFixture = f.prepare('bin-not-exist')

    await linkBins(path.join(binNotExistFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn: () => {},
      preferSymlinkedExecutables: true,
    })

    if (IS_WINDOWS) {
      // cmdShim
      expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins([]))
    } else {
      // it will fix symlink file permission
      expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['meow']))
    }
    expect(
      globalWarn
    ).toHaveBeenCalled()
  })
})

describe('node binary linking', () => {
  if (!IS_WINDOWS) {
    test('linkBinsOfPackages() symlinks node binary directly instead of creating a shell shim', async () => {
      const binTarget = temporaryDirectory()
      const nodeDir = temporaryDirectory()

      const nodeBinDir = path.join(nodeDir, 'bin')
      fs.mkdirSync(nodeBinDir, { recursive: true })
      fs.writeFileSync(path.join(nodeBinDir, 'node'), 'fake-node-binary', 'utf8')

      await linkBinsOfPackages(
        [
          {
            location: nodeDir,
            manifest: {
              name: 'node',
              version: '20.0.0',
              bin: { node: 'bin/node' },
            },
          },
        ],
        binTarget
      )

      const binLocation = path.join(binTarget, 'node')
      const stat = fs.lstatSync(binLocation)
      expect(stat.isSymbolicLink()).toBe(true)
      expect(fs.realpathSync(binLocation)).toBe(path.join(nodeBinDir, 'node'))
    })

    test('linkBinsOfPackages() replaces a dangling symlink when linking node binary', async () => {
      const binTarget = temporaryDirectory()
      const nodeDir = temporaryDirectory()

      const nodeBinDir = path.join(nodeDir, 'bin')
      fs.mkdirSync(nodeBinDir, { recursive: true })
      fs.writeFileSync(path.join(nodeBinDir, 'node'), 'fake-node-binary', 'utf8')

      // Create a dangling symlink at the target path (simulates a previous
      // node install whose store entry was removed).
      const binLocation = path.join(binTarget, 'node')
      fs.mkdirSync(binTarget, { recursive: true })
      const danglingTarget = path.join(temporaryDirectory(), 'non-existent-target')
      fs.symlinkSync(danglingTarget, binLocation)
      // Verify it's dangling: lstat succeeds but existsSync returns false
      expect(fs.lstatSync(binLocation).isSymbolicLink()).toBe(true)
      expect(fs.existsSync(binLocation)).toBe(false)

      await linkBinsOfPackages(
        [
          {
            location: nodeDir,
            manifest: {
              name: 'node',
              version: '20.0.0',
              bin: { node: 'bin/node' },
            },
          },
        ],
        binTarget
      )

      const stat = fs.lstatSync(binLocation)
      expect(stat.isSymbolicLink()).toBe(true)
      expect(fs.realpathSync(binLocation)).toBe(path.join(nodeBinDir, 'node'))
    })
  }

  testOnWindows('linkBinsOfPackages() hardlinks node.exe instead of creating a cmd-shim', async () => {
    const binTarget = temporaryDirectory()
    const nodeDir = temporaryDirectory()

    fs.writeFileSync(path.join(nodeDir, 'node.exe'), 'fake-node-binary', 'utf8')

    await linkBinsOfPackages(
      [
        {
          location: nodeDir,
          manifest: {
            name: 'node',
            version: '20.0.0',
            bin: { node: 'node.exe' },
          },
        },
      ],
      binTarget
    )

    const exePath = path.join(binTarget, 'node.exe')
    expect(fs.existsSync(exePath)).toBe(true)
    // Should be a hardlink, not a shim — same content as the original
    expect(fs.readFileSync(exePath, 'utf8')).toBe('fake-node-binary')
    // No cmd-shim should be created since we return early
    expect(fs.existsSync(path.join(binTarget, `node${CMD_EXTENSION}`))).toBe(false)
  })
})

test('linkBins() resolves conflicts using BIN_OWNER_OVERRIDES (npx owned by npm)', async () => {
  const binTarget = temporaryDirectory()
  const binOwnerOverrideFixture = f.prepare('bin-owner-override')
  const warn = jest.fn()

  await linkBins(binOwnerOverrideFixture, binTarget, { warn })

  // npx should be linked from npm package (owner override), not node or other-pkg
  // BIN_OWNER_OVERRIDES says: npx is owned by npm
  expect(binsConflictLogger.debug).toHaveBeenCalledWith(
    expect.objectContaining({
      binaryName: 'npx',
      binsDir: binTarget,
      linkedPkgName: 'npm',
      skippedPkgName: expect.any(String),
      skippedPkgVersion: expect.any(String),
    })
  )

  const binLocation = path.join(binTarget, 'npx')
  expect(fs.existsSync(binLocation)).toBe(true)
  const content = fs.readFileSync(binLocation, 'utf8')
  // npx should come from npm package, not node or other-pkg
  // Use a regex that matches both forward and backslashes for Windows compatibility
  expect(content).toMatch(/npm[/\\]bin[/\\]npx-cli\.js/)
})
