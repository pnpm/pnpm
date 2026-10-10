import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { tempDir } from '@pnpm/prepare'

function fixture () {
  const root = tempDir()
  const source = path.join(root, 'source')
  const target = path.join(root, 'target')
  fs.mkdirSync(path.join(source, 'lib'), { recursive: true })
  fs.writeFileSync(path.join(source, 'package.json'), '{}')
  fs.writeFileSync(path.join(source, 'lib/index.js'), 'original')
  const options = {
    filesMap: new Map([
      ['package.json', path.join(source, 'package.json')],
      ['lib/index.js', path.join(source, 'lib/index.js')],
    ]),
    force: false,
    resolvedFrom: 'local-dir' as const,
    keepModulesDir: true,
  }
  const importer = createIndexedPkgImporter('hardlink')
  const install = () => importer(target, options)
  install()
  return { source, target, options, install }
}

test('unchanged injected hardlinks preserve the package directory and dependencies', () => {
  const { target, install } = fixture()
  fs.mkdirSync(path.join(target, 'node_modules/dependency'), { recursive: true })
  fs.writeFileSync(path.join(target, 'node_modules/dependency/index.js'), 'dependency')
  const before = fs.statSync(target, { bigint: true })
  expect(install()).toBe('hardlink')
  expect(fs.statSync(target, { bigint: true }).ino).toBe(before.ino)
  expect(fs.readFileSync(path.join(target, 'node_modules/dependency/index.js'), 'utf8')).toBe('dependency')
})

test('injected refresh detects atomic replacements with unchanged size and mtime', () => {
  const { source, target, install } = fixture()
  const sourceFile = path.join(source, 'lib/index.js')
  fs.utimesSync(sourceFile, 1700000000, 1700000000)
  const original = fs.statSync(sourceFile, { bigint: true })
  const replacement = path.join(source, 'replacement')
  fs.writeFileSync(replacement, 'modified')
  fs.utimesSync(replacement, 1700000000, 1700000000)
  const replaced = fs.statSync(replacement, { bigint: true })
  expect(replaced.mtimeNs).toBe(original.mtimeNs)
  expect(replaced.size).toBe(original.size)
  expect(replaced.ino).not.toBe(original.ino)
  fs.renameSync(replacement, path.join(source, 'lib/index.js'))
  expect(fs.readFileSync(path.join(target, 'lib/index.js'), 'utf8')).toBe('original')
  expect(install()).toBe('hardlink')
  expect(fs.readFileSync(path.join(target, 'lib/index.js'), 'utf8')).toBe('modified')
})

test('injected refresh removes stale entries and restores missing files', () => {
  const { source, target, install, options } = fixture()
  fs.mkdirSync(path.join(target, 'stale-empty-dir'))
  fs.writeFileSync(path.join(target, 'removed.js'), 'removed')
  fs.rmSync(path.join(target, 'lib/index.js'))
  expect(install()).toBe('hardlink')
  expect(fs.existsSync(path.join(target, 'stale-empty-dir'))).toBe(false)
  expect(fs.existsSync(path.join(target, 'removed.js'))).toBe(false)
  expect(fs.readFileSync(path.join(target, 'lib/index.js'), 'utf8')).toBe('original')
  fs.writeFileSync(path.join(source, 'added.js'), 'added')
  options.filesMap.set('added.js', path.join(source, 'added.js'))
  install()
  expect(fs.readFileSync(path.join(target, 'added.js'), 'utf8')).toBe('added')
  options.filesMap.delete('lib/index.js')
  install()
  expect(fs.existsSync(path.join(target, 'lib'))).toBe(false)
})

test('same-byte copies and explicit force still recreate injected hardlinks', () => {
  const { source, target, install, options } = fixture()
  fs.rmSync(path.join(target, 'lib/index.js'))
  fs.copyFileSync(path.join(source, 'lib/index.js'), path.join(target, 'lib/index.js'))
  expect(install()).toBe('hardlink')
  expect(fs.statSync(path.join(target, 'lib/index.js'), { bigint: true }).ino)
    .toBe(fs.statSync(path.join(source, 'lib/index.js'), { bigint: true }).ino)
  const originalDirectory = fs.statSync(target, { bigint: true }).ino
  options.force = true
  expect(install()).toBe('hardlink')
  expect(fs.statSync(target, { bigint: true }).ino).not.toBe(originalDirectory)
  const copy = createIndexedPkgImporter('copy')
  expect(copy(target, { ...options, force: false })).toBe('copy')
  expect(fs.statSync(path.join(target, 'lib/index.js'), { bigint: true }).ino)
    .not.toBe(fs.statSync(path.join(source, 'lib/index.js'), { bigint: true }).ino)
})

test('injected refresh replaces directory symlinks without modifying their targets', () => {
  const { source, target, install } = fixture()
  fs.rmSync(path.join(target, 'lib'), { recursive: true })
  fs.symlinkSync(path.join(source, 'lib'), path.join(target, 'lib'), 'junction')
  expect(install()).toBe('hardlink')
  expect(fs.lstatSync(path.join(target, 'lib')).isSymbolicLink()).toBe(false)
  expect(fs.readFileSync(path.join(source, 'lib/index.js'), 'utf8')).toBe('original')
})


test('injected refresh replaces a symlink at the package root', () => {
  const { source, target, install } = fixture()
  fs.rmSync(target, { recursive: true })
  fs.symlinkSync(source, target, 'junction')
  expect(install()).toBe('hardlink')
  expect(fs.lstatSync(target).isSymbolicLink()).toBe(false)
  expect(fs.readFileSync(path.join(source, 'package.json'), 'utf8')).toBe('{}')
})

test('bundled dependencies prevent injected hardlink reuse', () => {
  const { source, target, install, options } = fixture()
  const bundled = path.join(source, 'node_modules/bundled/index.js')
  fs.mkdirSync(path.dirname(bundled), { recursive: true })
  fs.writeFileSync(bundled, 'bundled')
  options.filesMap.set('node_modules/bundled/index.js', bundled)
  expect(install()).toBe('hardlink')
  expect(install()).toBe('hardlink')
  expect(fs.readFileSync(path.join(target, 'node_modules/bundled/index.js'), 'utf8')).toBe('bundled')
})
