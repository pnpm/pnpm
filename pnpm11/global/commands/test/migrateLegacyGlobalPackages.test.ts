import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'

import { isLegacyBin, legacyBinFiles, legacyMigrationSelectors, readLegacyGlobalLayout, removeLegacyGlobalLayout } from '../src/migrateLegacyGlobalPackages.js'

test('readLegacyGlobalLayout returns null without a legacy manifest', async () => {
  const globalRoot = path.join(tempDir(), 'global')
  fs.mkdirSync(path.join(globalRoot, '5'), { recursive: true })

  expect(await readLegacyGlobalLayout(path.join(globalRoot, 'v11'))).toBeNull()
})

test('selectors skip pnpm and the packages a current group installs', async () => {
  const globalRoot = path.join(tempDir(), 'global')
  writeLegacyManifest(globalRoot, {
    typescript: '^5.4.0',
    pnpm: '^10.0.0',
    '@pnpm/exe': '10.0.0',
    pm: 'npm:pnpm@10',
    eslint: '9.0.0',
  })
  const legacy = (await readLegacyGlobalLayout(path.join(globalRoot, 'v11')))!

  expect(legacyMigrationSelectors(legacy, new Set(['eslint']))).toStrictEqual([
    { alias: 'typescript', selector: 'typescript@^5.4.0' },
  ])
})

test('selectors anchor relative local paths at the legacy project', async () => {
  const globalRoot = path.join(tempDir(), 'global')
  const legacyDir = writeLegacyManifest(globalRoot, {
    'my-tool': 'link:../../tools/my-tool',
    'other-tool': 'file:/tools/other-tool',
  })
  const legacy = (await readLegacyGlobalLayout(path.join(globalRoot, 'v11')))!

  const selectors = legacyMigrationSelectors(legacy, new Set()).sort((a, b) => a.alias.localeCompare(b.alias))

  expect(selectors).toStrictEqual([
    { alias: 'my-tool', selector: `my-tool@link:${path.resolve(legacyDir, '../../tools/my-tool')}` },
    { alias: 'other-tool', selector: 'other-tool@file:/tools/other-tool' },
  ])
})

test('isLegacyBin accepts a shim that runs the legacy project relative to itself', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const shim = path.join(home, 'tsc')
  fs.writeFileSync(shim, '#!/bin/sh\nexec node "$basedir/global/5/node_modules/typescript/bin/tsc" "$@"\n')

  expect(await isLegacyBin(shim, legacyDir)).toBe(true)
})

test('isLegacyBin accepts a cmd shim with backslashes and an absolute target', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const shim = path.join(home, 'tsc.cmd')
  const target = path.join(legacyDir, 'node_modules', 'typescript', 'bin', 'tsc').replaceAll('/', '\\')
  fs.writeFileSync(shim, `@node "${target}" %*\r\n`)

  expect(await isLegacyBin(shim, legacyDir)).toBe(true)
})

test('isLegacyBin accepts a symlink into the legacy project', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const link = path.join(home, 'tsc')
  fs.symlinkSync(path.join('global', '5', 'node_modules', 'typescript', 'bin', 'tsc'), link)

  expect(await isLegacyBin(link, legacyDir)).toBe(true)
})

test('isLegacyBin accepts a link whose missing target is reached through a symlinked directory', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const alias = path.join(path.dirname(home), 'home-alias')
  fs.symlinkSync(home, alias, 'dir')
  const link = path.join(home, 'tsc')
  fs.symlinkSync(path.join(alias, 'global', '5', 'node_modules', 'typescript', 'bin', 'tsc'), link)

  expect(await isLegacyBin(link, legacyDir)).toBe(true)
})

test('isLegacyBin keeps a shim of the current layout and an unrelated file', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const current = path.join(home, 'tsc')
  fs.writeFileSync(current, '#!/bin/sh\nexec node "$basedir/global/v11/abc/node_modules/typescript/bin/tsc" "$@"\n')
  const unrelated = path.join(home, 'my-script')
  fs.writeFileSync(unrelated, '#!/bin/sh\necho hi\n')

  expect(await isLegacyBin(current, legacyDir)).toBe(false)
  expect(await isLegacyBin(unrelated, legacyDir)).toBe(false)
  expect(await isLegacyBin(path.join(home, 'missing'), legacyDir)).toBe(false)
})

test('legacyBinFiles lists only the files that point into the legacy project', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  fs.writeFileSync(path.join(home, 'tool.cmd'), '@node "%~dp0\\global\\5\\node_modules\\tool\\cli.js" %*\r\n')
  fs.writeFileSync(path.join(home, 'tool.exe'), 'MZ')
  fs.writeFileSync(path.join(home, 'tool'), '#!/bin/sh\necho mine\n')

  expect(await legacyBinFiles(path.join(home, 'tool'), legacyDir)).toStrictEqual([path.join(home, 'tool.cmd')])
})

test('legacyBinFiles identifies hard links by identity', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const target = path.join(legacyDir, 'node_modules', 'tool.exe')
  fs.writeFileSync(target, Buffer.alloc(128 * 1024, 0xff))
  const bin = path.join(home, 'tool.exe')
  fs.linkSync(target, bin)
  const copy = path.join(home, 'copy.exe')
  fs.copyFileSync(target, copy)
  fs.writeFileSync(`${bin}.cmd`, 'user script')

  expect(await legacyBinFiles(bin, legacyDir, target)).toStrictEqual([bin])
  expect(await legacyBinFiles(path.join(home, 'tool'), legacyDir, target)).toStrictEqual([bin])
  expect(await legacyBinFiles(copy, legacyDir, target)).toStrictEqual([])
  expect(await legacyBinFiles(copy, legacyDir, path.join(legacyDir, 'missing'))).toStrictEqual([])
})

test('legacyBinFiles preserves a home executable reached through a package symlink', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const bin = path.join(home, 'tool')
  fs.writeFileSync(bin, 'user executable')
  const target = path.join(legacyDir, 'node_modules', 'tool')
  fs.symlinkSync(bin, target)

  expect(await legacyBinFiles(bin, legacyDir, target)).toStrictEqual([])
})

test('legacyBinFiles preserves a home executable reached through a directory symlink', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const bin = path.join(home, 'tool')
  fs.writeFileSync(bin, 'user executable')
  const linkedDir = path.join(legacyDir, 'node_modules', 'linked')
  fs.symlinkSync(home, linkedDir, 'junction')

  expect(await legacyBinFiles(bin, legacyDir, path.join(linkedDir, 'tool'))).toStrictEqual([])
})

test.each(['first', 'second'])('migration cleans up a shared bin linked to the %s package', async (owner) => {
  const { home, legacyDir } = homeWithLegacyDir()
  const dependencies = { first: '1.0.0', second: '1.0.0' }
  for (const name of Object.keys(dependencies)) {
    const pkgDir = path.join(legacyDir, 'node_modules', name)
    fs.mkdirSync(pkgDir)
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name, version: '1.0.0', bin: { tool: 'cli.js' } }))
    fs.writeFileSync(path.join(pkgDir, 'cli.js'), name)
  }
  const bin = path.join(home, 'tool')
  fs.linkSync(path.join(legacyDir, 'node_modules', owner, 'cli.js'), bin)
  const readFile = fs.promises.readFile.bind(fs.promises)
  const spy = jest.spyOn(fs.promises, 'readFile').mockImplementation(async (...args: Parameters<typeof fs.promises.readFile>) => {
    if (args[0] === bin) await new Promise((resolve) => setTimeout(resolve, 30))
    return readFile(...args)
  })
  try {
    await removeLegacyGlobalLayout({ dir: legacyDir, dependencies }, home)
    expect(fs.existsSync(bin)).toBe(false)
    expect(fs.existsSync(legacyDir)).toBe(false)
  } finally {
    spy.mockRestore()
  }
})

test('migration keeps the legacy layout when a bin target cannot be inspected', async () => {
  const { home, legacyDir } = homeWithLegacyDir()
  const pkgDir = path.join(legacyDir, 'node_modules', 'tool')
  fs.mkdirSync(pkgDir)
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'tool', version: '1.0.0', bin: 'cli.js' }))
  fs.symlinkSync('cli.js', path.join(pkgDir, 'cli.js'))
  const bin = path.join(home, 'tool')
  fs.writeFileSync(bin, 'user executable')

  await removeLegacyGlobalLayout({ dir: legacyDir, dependencies: { tool: '1.0.0' } }, home)

  expect(fs.existsSync(legacyDir)).toBe(true)
  expect(fs.readFileSync(bin, 'utf8')).toBe('user executable')
})

function tempDir (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-legacy-global-'))
}

function writeLegacyManifest (globalRoot: string, dependencies: Record<string, string>): string {
  const legacyDir = path.join(globalRoot, '5')
  fs.mkdirSync(legacyDir, { recursive: true })
  fs.writeFileSync(path.join(legacyDir, 'package.json'), JSON.stringify({ dependencies }))
  return legacyDir
}

function homeWithLegacyDir (): { home: string, legacyDir: string } {
  const home = path.join(tempDir(), 'pnpm-home')
  const legacyDir = path.join(home, 'global', '5')
  fs.mkdirSync(path.join(legacyDir, 'node_modules'), { recursive: true })
  return { home, legacyDir }
}
