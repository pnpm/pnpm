/// <reference path="../../../__typings__/index.d.ts"/>
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, jest, test } from '@jest/globals'
import { cmdShim } from '@pnpm/bins.cmd-shim'
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
  getBinsToLink,
  linkBins,
  linkBinsOfPackages,
  linkBinsOfPkgsByAliases,
} = await import('@pnpm/bins.linker')

const binsConflictLogger = logger('bins-conflict')
const PRINTF_BASEDIR_LINE = String.raw`basedir=$(command -p printf '%s\n' "$link" | command -p sed -e 's,\\,/,g')`
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
const testOnPosix = IS_WINDOWS ? test.skip : test
// Root reads through a directory whose permissions deny access.
const testOnPosixAsNonRoot = IS_WINDOWS || process.getuid?.() === 0 ? test.skip : test

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
  expect(originalContent).toContain(PRINTF_BASEDIR_LINE)
  // Append a sentinel to the existing (correct) content to prove it is not rewritten
  const sentinel = originalContent + '\n# sentinel'
  fs.writeFileSync(binLocation, sentinel, 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(fs.readFileSync(binLocation, 'utf8')).toBe(sentinel)
})

test('linkBins() puts projectModulesDir first on NODE_PATH, then the bin\'s own directories, then extraNodePaths', async () => {
  const warn = jest.fn()
  const modulesDir = path.join(f.prepare('simple-fixture'), 'node_modules')
  const hoisted = path.join(modulesDir, '.pnpm', 'node_modules')

  const binTarget = temporaryDirectory()
  await linkBins(modulesDir, binTarget, { warn, extraNodePaths: [hoisted], projectModulesDir: path.join(modulesDir, '..', 'vendor') })
  const entries = nodePathEntries(fs.readFileSync(path.join(binTarget, 'simple'), 'utf8'))
  expect(entries).toHaveLength(4)
  expect(entries[0]).toMatch(/\/vendor$/)
  expect(entries[1]).toMatch(/\/node_modules\/simple\/node_modules$/)
  expect(entries[3]).toMatch(/\/node_modules\/\.pnpm\/node_modules$/)

  const dedupedTarget = temporaryDirectory()
  const realModulesDir = fs.realpathSync(modulesDir)
  await linkBins(modulesDir, dedupedTarget, { warn, extraNodePaths: [realModulesDir], projectModulesDir: realModulesDir })
  const deduped = nodePathEntries(fs.readFileSync(path.join(dedupedTarget, 'simple'), 'utf8'))
  expect(deduped).toHaveLength(2)
  expect(deduped[0]).toBe(nodePathEntries(fs.readFileSync(path.join(binTarget, 'simple'), 'utf8'))[2])
  expect(deduped[1]).toMatch(/\/node_modules\/simple\/node_modules$/)

  fs.appendFileSync(path.join(dedupedTarget, 'simple'), '# sentinel\n')
  await linkBins(modulesDir, dedupedTarget, { warn, extraNodePaths: [realModulesDir], projectModulesDir: realModulesDir })
  expect(fs.readFileSync(path.join(dedupedTarget, 'simple'), 'utf8')).toContain('# sentinel')
})

test('linkBins() keeps or rewrites the NODE_PATH of an existing bin according to its options', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const modulesDir = path.join(f.prepare('simple-fixture'), 'node_modules')
  const binLocation = path.join(binTarget, 'simple')
  const extraNodePaths = [path.join(modulesDir, '.pnpm', 'node_modules')]
  const projectModulesDir = path.join(modulesDir, '..', 'vendor')
  // The sentinel survives only when linkBins() leaves the bin in place.
  const relink = async (opts: { extraNodePaths?: string[], projectModulesDir?: string }): Promise<{ kept: boolean, entries: string[] }> => {
    fs.appendFileSync(binLocation, '# sentinel\n')
    await linkBins(modulesDir, binTarget, { warn, ...opts })
    const content = fs.readFileSync(binLocation, 'utf8')
    return { kept: content.includes('# sentinel'), entries: nodePathEntries(content) }
  }

  await linkBins(modulesDir, binTarget, { warn, extraNodePaths })

  const withProject = await relink({ extraNodePaths, projectModulesDir })
  expect(withProject.kept).toBe(false)
  expect(withProject.entries[0]).toMatch(/\/vendor$/)
  expect(await relink({ extraNodePaths, projectModulesDir })).toMatchObject({ kept: true })
  expect(await relink({ projectModulesDir })).toMatchObject({ kept: true })
  expect(await relink({ extraNodePaths })).toMatchObject({ kept: true })
  expect(await relink({})).toMatchObject({ kept: true, entries: withProject.entries })

  expect(await relink({ extraNodePaths: [] })).toStrictEqual({ kept: false, entries: [] })

  const projectWithoutExtras = await relink({ projectModulesDir })
  expect(projectWithoutExtras.kept).toBe(false)
  expect(projectWithoutExtras.entries[0]).toMatch(/\/vendor$/)
})

function nodePathEntries (shim: string): string[] {
  return /^ {2}export NODE_PATH="([^"]*)"$/m.exec(shim)?.[1].split(':') ?? []
}

// A shim an older pnpm wrote still points at the right target, so the warm
// install path had nothing to notice and left it in place. It resolved
// readlink and its other helpers on the caller's PATH, which starts with the
// very directory the shim lives in, so upgrading pnpm has to replace it.
test('linkBins() replaces a shim that looks its helpers up on PATH', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')
  const target = normalizePath(path.join(simpleFixture, 'node_modules', 'simple', 'index.js'))

  fs.mkdirSync(binTarget, { recursive: true })
  const binLocation = path.join(binTarget, 'simple')
  const outdated = `#!/bin/sh
link="$0"
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="$(dirname "$link")/$target" ;;
  esac
done
basedir=$(dirname "$(echo "$link" | sed -e 's,\\\\,/,g')")
exec node  "$basedir/../simple/index.js" "$@"
# cmd-shim-target=${target}
`
  fs.writeFileSync(binLocation, outdated, 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toContain(`# cmd-shim-target=${target}\n`)
  expect(content).toContain('  target=$(command -p readlink "$link")\n')
})

test('linkBins() replaces a shim that still pipes the path through echo', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')
  const target = normalizePath(path.join(simpleFixture, 'node_modules', 'simple', 'index.js'))

  fs.mkdirSync(binTarget, { recursive: true })
  const binLocation = path.join(binTarget, 'simple')
  const outdated = `#!/bin/sh
link="$0"
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(command -p readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="$(dirname "$link")/$target" ;;
  esac
done
basedir=$(echo "$link" | command -p sed -e 's,\\\\,/,g')
exec node  "$basedir/../simple/index.js" "$@"
# cmd-shim-target=${target}
# outdated-echo-basedir
`
  fs.writeFileSync(binLocation, outdated, 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toContain(`# cmd-shim-target=${target}\n`)
  expect(content).toContain('  target=$(command -p readlink "$link")\n')
  expect(content).toContain(PRINTF_BASEDIR_LINE)
  expect(content).not.toContain('# outdated-echo-basedir')
})

test('linkBins() replaces a shim that converts Windows paths with a helper from PATH', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')
  const target = normalizePath(path.join(simpleFixture, 'node_modules', 'simple', 'index.js'))

  fs.mkdirSync(binTarget, { recursive: true })
  const binLocation = path.join(binTarget, 'simple')
  const outdated = `#!/bin/sh
link="$0"
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(command -p readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="\${link%/*}/$target" ;;
  esac
done
${PRINTF_BASEDIR_LINE}
basedir="\${basedir%/*}"
basedir_win="$basedir"
exe=""
msys=""

case \`command -p uname -a\` in
  *CYGWIN*|*MINGW*|*MSYS*)
    if command -v cygpath > /dev/null 2>&1; then
      basedir_win=\`cygpath -w "$basedir"\`
    fi
    exe=".exe"
    msys="true"
  ;;
  *WSL2*)
    if command -v wslpath > /dev/null 2>&1; then
      basedir_win="$(wslpath -w "$basedir" 2> /dev/null)"
    fi
  ;;
esac

exec node  "$basedir/../simple/index.js" "$@"
# cmd-shim-target=${target}
# outdated-path-converters
`
  fs.writeFileSync(binLocation, outdated, 'utf8')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  const content = fs.readFileSync(binLocation, 'utf8')
  expect(content).toContain(`# cmd-shim-target=${target}\n`)
  expect(content).toContain('    if converted=$(command -p cygpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then\n')
  expect(content).toContain('    if converted=$(command -p wslpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then\n')
  expect(content).not.toContain('# outdated-path-converters')
})

testOnPosix('linkBins() repairs a non-executable source when the existing bin references it', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')
  const binSource = path.join(simpleFixture, 'node_modules', 'simple', 'index.js')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })
  fs.chmodSync(binSource, 0o644)

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })

  expect(fs.statSync(binSource).mode & 0o777).toBe(0o755)
})

testOnPosix('linkBins() keeps a correctly linked bin whose source file is missing', async () => {
  const binTarget = temporaryDirectory()
  const warn = jest.fn()
  const simpleFixture = f.prepare('simple-fixture')
  const binSource = path.join(simpleFixture, 'node_modules', 'simple', 'index.js')

  await linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })
  fs.rmSync(binSource)

  await expect(linkBins(path.join(simpleFixture, 'node_modules'), binTarget, { warn })).resolves.not.toThrow()
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

test('linkBins() deletes a PowerShell shim left by an older install of the pnpm CLI', async () => {
  const binTarget = temporaryDirectory()
  const fixture = f.prepare('pnpm-cli')
  const warn = jest.fn()

  fs.mkdirSync(binTarget, { recursive: true })
  for (const binName of ['pnpm', 'pn']) {
    fs.writeFileSync(path.join(binTarget, `${binName}.ps1`), 'an older install wrote this')
  }

  await linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })

  const bins = fs.readdirSync(binTarget)
  for (const binName of ['pnpm', 'pn']) {
    expect(bins).toContain(binName)
    expect(bins).not.toContain(`${binName}.ps1`)
    if (IS_WINDOWS) {
      expect(bins).toContain(`${binName}${CMD_EXTENSION}`)
    }
  }

  // The warm-relink short-circuit must not let a .ps1 planted after the first
  // link survive.
  fs.writeFileSync(path.join(binTarget, 'pnpm.ps1'), 'planted after the first link')
  await linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })
  expect(fs.readdirSync(binTarget)).not.toContain('pnpm.ps1')
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

  const packages = [
    {
      location: path.join(modulesPath, 'bar'),
      manifest: (await import(path.join(modulesPath, 'bar', 'package.json'))).default,
    },
    {
      location: path.join(modulesPath, 'foo'),
      manifest: (await import(path.join(modulesPath, 'foo', 'package.json'))).default,
    },
  ]
  const binsToLink = await getBinsToLink(packages)

  expect(binsToLink.find(({ name }) => name === 'bar')?.path).toBe(path.join(modulesPath, 'bar/index.js'))

  await linkBinsOfPackages(packages, binTarget)

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

test("linkBins() creates a bin that points to a path that doesn't exist yet", async () => {
  const binTarget = temporaryDirectory()
  const binNotExistFixture = f.prepare('bin-not-exist')

  await linkBins(path.join(binNotExistFixture, 'node_modules'), binTarget, {
    allowExoticManifests: true,
    warn: () => {},
  })

  expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['meow']))
  expect(globalWarn).not.toHaveBeenCalled()
  if (IS_WINDOWS) {
    expect(fs.readFileSync(path.join(binTarget, `meow${CMD_EXTENSION}`), 'utf8')).toMatch('node')
  }

  const binSource = path.join(binNotExistFixture, 'node_modules', 'foo', 'dist', 'not-exist.js')
  fs.mkdirSync(path.dirname(binSource), { recursive: true })
  fs.writeFileSync(binSource, 'console.log(\'built\')\n')
  const shim = path.join(binTarget, IS_WINDOWS ? `meow${CMD_EXTENSION}` : 'meow')
  const result = spawnSync(IS_WINDOWS ? `"${shim}"` : shim, { shell: IS_WINDOWS })
  expect(result.stdout.toString()).toMatch('built')
})

test('linkBinsOfPackages() rewrites a shim written for a missing target once the target exists', async () => {
  const pkgDir = temporaryDirectory()
  const binsDir = temporaryDirectory()
  const pkg = { location: pkgDir, manifest: { name: 'tool', version: '1.0.0', bin: 'bin/tool' } }

  await linkBinsOfPackages([pkg], binsDir)
  expect(fs.readFileSync(path.join(binsDir, 'tool'), 'utf8')).not.toMatch(/exec node /)

  fs.mkdirSync(path.join(pkgDir, 'bin'))
  fs.writeFileSync(path.join(pkgDir, 'bin', 'tool'), '#!/usr/bin/env node\nconsole.log(\'built\')\n')
  await linkBinsOfPackages([pkg], binsDir)

  expect(fs.readFileSync(path.join(binsDir, 'tool'), 'utf8')).toMatch(/exec node +"\$basedir\//)
  if (IS_WINDOWS) {
    expect(fs.readFileSync(path.join(binsDir, `tool${CMD_EXTENSION}`), 'utf8')).toMatch('node')
  }
})

test("linkBinsOfPackages() does not link a package's missing bin into its own .bin directory", async () => {
  const binNotExistFixture = f.prepare('bin-not-exist')
  const pkgDir = path.join(binNotExistFixture, 'node_modules', 'foo')
  const ownBinsDir = path.join(pkgDir, 'node_modules', '.bin')
  const pkg = {
    location: pkgDir,
    manifest: JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf8')),
  }

  await linkBinsOfPackages([pkg], ownBinsDir)

  expect(fs.readdirSync(ownBinsDir)).toEqual([])

  const binSource = path.join(pkgDir, 'dist', 'not-exist.js')
  fs.mkdirSync(path.dirname(binSource), { recursive: true })
  fs.writeFileSync(binSource, 'console.log(\'built\')\n')

  await linkBinsOfPackages([pkg], ownBinsDir)

  expect(fs.readdirSync(ownBinsDir)).toEqual(getExpectedBins(['meow']))

  fs.rmSync(binSource)
  await linkBinsOfPackages([pkg], ownBinsDir)

  expect(fs.readdirSync(ownBinsDir)).toEqual([])
})

testOnWindows("linkBinsOfPackages() links a package's own bin whose target exists only with an .exe extension", async () => {
  const pkgDir = temporaryDirectory()
  const ownBinsDir = path.join(pkgDir, 'node_modules', '.bin')
  fs.mkdirSync(path.join(pkgDir, 'bin'))
  fs.writeFileSync(path.join(pkgDir, 'bin', 'tool.exe'), '')

  await linkBinsOfPackages([{ location: pkgDir, manifest: { name: 'tool', version: '1.0.0', bin: 'bin/tool' } }], ownBinsDir)

  expect(fs.readdirSync(ownBinsDir)).toEqual(getExpectedBins(['tool']))
})

testOnPosixAsNonRoot("linkBinsOfPackages() links a package's other own bins when probing one of them fails", async () => {
  const pkgDir = temporaryDirectory()
  const ownBinsDir = path.join(pkgDir, 'node_modules', '.bin')
  const lockedDir = path.join(pkgDir, 'locked')
  fs.mkdirSync(lockedDir)
  fs.writeFileSync(path.join(pkgDir, 'ok.js'), 'console.log(\'ok\')\n')
  fs.chmodSync(lockedDir, 0o000)
  try {
    await expect(linkBinsOfPackages([{
      location: pkgDir,
      manifest: { name: 'tool', version: '1.0.0', bin: { locked: 'locked/tool.js', ok: 'ok.js' } },
    }], ownBinsDir)).rejects.toHaveProperty('code', 'EACCES')
  } finally {
    fs.chmodSync(lockedDir, 0o755)
  }

  expect(fs.readdirSync(ownBinsDir)).toEqual(['ok'])
})

testOnWindows("linkBinsOfPackages() keeps a bin named like the .cmd sibling of a package's own missing bin", async () => {
  const pkgDir = temporaryDirectory()
  const ownBinsDir = path.join(pkgDir, 'node_modules', '.bin')
  fs.mkdirSync(path.join(pkgDir, 'bin'))
  fs.writeFileSync(path.join(pkgDir, 'bin', 'cli.js'), 'console.log(\'cli\')\n')

  await linkBinsOfPackages([{
    location: pkgDir,
    manifest: { name: 'tool', version: '1.0.0', bin: { tool: 'bin/missing.js', 'tool.cmd': 'bin/cli.js' } },
  }], ownBinsDir)

  expect(fs.readFileSync(path.join(ownBinsDir, 'tool.cmd'), 'utf8')).toMatch('cli.js')
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

  test("linkBins() creates a bin that points to a path that doesn't exist yet", async () => {
    const binTarget = temporaryDirectory()
    const binNotExistFixture = f.prepare('bin-not-exist')

    await linkBins(path.join(binNotExistFixture, 'node_modules'), binTarget, {
      allowExoticManifests: true,
      warn: () => {},
      preferSymlinkedExecutables: true,
    })

    expect(fs.readdirSync(binTarget)).toEqual(getExpectedBins(['meow']))
    expect(globalWarn).not.toHaveBeenCalled()
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

  testOnWindows('linkBinsOfPackages() does not warn when node.exe is already the correct hardlink', async () => {
    const binTarget = temporaryDirectory()
    const nodeDir = temporaryDirectory()

    fs.writeFileSync(path.join(nodeDir, 'node.exe'), 'fake-node-binary', 'utf8')

    const pkgs = [
      {
        location: nodeDir,
        manifest: {
          name: 'node',
          version: '20.0.0',
          bin: { node: 'node.exe' },
        },
      },
    ]

    // First call creates the hardlink
    await linkBinsOfPackages(pkgs, binTarget)
    jest.mocked(globalWarn).mockClear()

    // Second call should not warn because the .exe is already the correct hardlink
    await linkBinsOfPackages(pkgs, binTarget)

    // The absence of a warning is the regression signal: the warn and the
    // `rimraf` that recreates node.exe live in the same branch, so no warning
    // means node.exe was left untouched. We don't assert on inode identity
    // because node.exe may legitimately be a copy rather than a hardlink (the
    // implementation falls back to copyFile when hardlinking fails).
    expect(globalWarn).not.toHaveBeenCalled()
    const exePath = path.join(binTarget, 'node.exe')
    expect(fs.existsSync(exePath)).toBe(true)
    expect(fs.readFileSync(exePath, 'utf8')).toBe('fake-node-binary')
  })

  testOnWindows('linkBinsOfPackages() does not warn when node.exe has identical content but is not a hardlink', async () => {
    const binTarget = temporaryDirectory()
    const nodeDir = temporaryDirectory()

    fs.writeFileSync(path.join(nodeDir, 'node.exe'), 'fake-node-binary', 'utf8')
    // Pre-place an independent copy with identical content (different inode), as
    // happens on filesystems where Windows reports a zero inode and the previous
    // link fell back to a copy.
    fs.writeFileSync(path.join(binTarget, 'node.exe'), 'fake-node-binary', 'utf8')

    const pkgs = [
      {
        location: nodeDir,
        manifest: {
          name: 'node',
          version: '20.0.0',
          bin: { node: 'node.exe' },
        },
      },
    ]

    await linkBinsOfPackages(pkgs, binTarget)

    expect(globalWarn).not.toHaveBeenCalled()
    const exePath = path.join(binTarget, 'node.exe')
    expect(fs.existsSync(exePath)).toBe(true)
    expect(fs.readFileSync(exePath, 'utf8')).toBe('fake-node-binary')
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

// The shell sets $0 to the invoked symlink, not the shim it points at, so a
// shim reached through external symlinks must follow the chain before
// deriving basedir (https://github.com/pnpm/pnpm/issues/13405).
testOnPosix('generated POSIX shim resolves symlink chains and executes its target', async () => {
  const projectDir = temporaryDirectory()
  const binDir = path.join(projectDir, 'node_modules', '.bin')
  const targetDir = path.join(projectDir, 'node_modules', 'typescript', 'bin')
  fs.mkdirSync(binDir, { recursive: true })
  fs.mkdirSync(targetDir, { recursive: true })

  const targetPath = path.join(targetDir, 'tsc')
  fs.writeFileSync(targetPath, '#!/bin/sh\necho "tsc-output"\n', 'utf8')
  fs.chmodSync(targetPath, 0o755)

  const shimPath = path.join(binDir, 'tsc')
  await cmdShim(targetPath, shimPath)

  // hop2's relative target exercises the shim's dirname-composition
  // branch; hop1's absolute target exercises the other.
  const hop1 = path.join(projectDir, 'symlink_hop_1')
  fs.symlinkSync(shimPath, hop1)
  const hop2Dir = path.join(projectDir, 'local', 'bin')
  fs.mkdirSync(hop2Dir, { recursive: true })
  const hop2 = path.join(hop2Dir, 'tsc')
  fs.symlinkSync(path.join('..', '..', 'symlink_hop_1'), hop2)

  const { status, stdout, stderr } = spawnSync(hop2, { encoding: 'utf8' })
  expect(stderr).toBe('')
  expect(status).toBe(0)
  expect(stdout.trim()).toBe('tsc-output')
})

// A shim runs with node_modules/.bin at the front of PATH, which is where a
// dependency's own bins live, so a helper taken from there could report any
// directory it liked and redirect what the shim finally execs
// (https://github.com/pnpm/pnpm/issues/14837).
describe('generated POSIX shim resolves its helpers off the caller\'s PATH', () => {
  function writeExecutable (file: string, body: string): void {
    fs.writeFileSync(file, body, 'utf8')
    fs.chmodSync(file, 0o755)
  }

  // A shimmed tool plus a relative symlink to it in the same directory, so the
  // walk composes a directory with the link target instead of taking one
  // straight from readlink.
  async function makeShimmedTool (projectDir: string): Promise<string> {
    const binDir = path.join(projectDir, 'node_modules', '.bin')
    const target = path.join(projectDir, 'node_modules', 'typescript', 'bin', 'tsc.js')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, 'console.log("tsc-output")\n', 'utf8')
    // A dependency can declare a bin named node.exe, and the shim's basedir is
    // the directory those bins land in. Only a lying uname reaches it.
    writeExecutable(path.join(binDir, 'node.exe'), '#!/bin/sh\necho hijacked\n')

    await cmdShim(target, path.join(binDir, 'tsc'), { createCmdFile: false })
    fs.symlinkSync('tsc', path.join(binDir, 'tsc-link'))
    return binDir
  }

  // Write the tree the decoys point at, and the decoys, returning the directory
  // to put at the front of PATH. Each decoy answers with what its real
  // counterpart would be asked for, so any one of them alone is enough to
  // redirect the shim.
  function plantHijackTreeAndDecoys (projectDir: string): string {
    const hijack = path.join(projectDir, 'hijack', 'node_modules')
    const hijackBin = path.join(hijack, '.bin')
    const hijackTarget = path.join(hijack, 'typescript', 'bin', 'tsc.js')
    fs.mkdirSync(hijackBin, { recursive: true })
    fs.mkdirSync(path.dirname(hijackTarget), { recursive: true })
    fs.writeFileSync(hijackTarget, 'console.log("hijacked")\n', 'utf8')

    const decoyDir = path.join(projectDir, 'decoy')
    fs.mkdirSync(decoyDir)
    const answer = (p: string) => `#!/bin/sh\necho '${p}'\n`
    for (const helper of ['readlink', 'sed']) {
      writeExecutable(path.join(decoyDir, helper), answer(path.join(hijackBin, 'tsc')))
    }
    writeExecutable(path.join(decoyDir, 'dirname'), answer(hijackBin))
    writeExecutable(path.join(decoyDir, 'uname'), '#!/bin/sh\necho MINGW64_NT-10.0\n')
    return decoyDir
  }

  function expectShimToReachItsTarget (projectDir: string, command: string, args: string[], cwd?: string): void {
    const decoyDir = plantHijackTreeAndDecoys(projectDir)
    const { status, stdout, stderr } = spawnSync(command, args, {
      cwd,
      encoding: 'utf8',
      env: {
        ...process.env,
        PATH: [decoyDir, path.dirname(process.execPath), process.env.PATH].join(path.delimiter),
      },
    })
    expect(stderr).toBe('')
    expect(status).toBe(0)
    expect(stdout.trim()).toBe('tsc-output')
  }

  testOnPosix('with decoy readlink, dirname, sed, and uname first on PATH', async () => {
    const projectDir = temporaryDirectory()
    const binDir = await makeShimmedTool(projectDir)

    expectShimToReachItsTarget(projectDir, path.join(binDir, 'tsc-link'), [])
  })

  // The kernel and the C library's PATH search hand the interpreter the path
  // they resolved, so $0 is bare only when a shell is given the name itself.
  testOnPosix('when sh receives a bare name', async () => {
    const projectDir = temporaryDirectory()
    const binDir = await makeShimmedTool(projectDir)

    expectShimToReachItsTarget(projectDir, 'sh', ['tsc-link'], binDir)
  })
})
