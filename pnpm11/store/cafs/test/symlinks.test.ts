import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import type { PackageFiles } from '@pnpm/store.cafs-types'
import { temporaryDirectory } from 'tempy'

import {
  buildFileMapsFromIndex,
  checkPkgFilesIntegrity,
  createCafs,
  normalizeSymlinkTarget,
  type PackageFilesIndex,
  SYMLINK_MODE,
} from '../src/index.js'

const itOnPosix = process.platform === 'win32' ? it.skip : it

describe('normalizeSymlinkTarget()', () => {
  it.each([
    ['bin/git-add', 'git', 'git'],
    ['bin/git-add', './git', 'git'],
    ['bin/lib', '../lib', '../lib'],
    ['a/b/c', '../../d/e', '../../d/e'],
    ['a/b/c', '..//d/./e', '../d/e'],
  ])('records the link at %s to %s as %s', (linkPath, target, expected) => {
    expect(normalizeSymlinkTarget(linkPath, target)).toBe(expected)
  })

  it.each([
    ['link', '../outside', 'climbs above the package root'],
    ['a/link', '../../outside', 'climbs above the package root'],
    ['link', '/etc/passwd', 'is absolute'],
    ['link', 'C:/Windows', 'is a Windows path'],
    ['link', 'dir\\file', 'contains a backslash'],
    ['link', 'a/../../outside', 'climbs after naming a directory'],
    ['a/link', '..', 'points at a directory containing the link'],
    ['link', '.', 'points at the package root'],
    ['link', 'node_modules/dep', 'names node_modules'],
    ['link', 'sub/node_modules/dep', 'names node_modules'],
    ['node_modules/link', 'target', 'is in the dependencies directory'],
    ['package.json', 'manifest.json', 'replaces the manifest'],
    ['PACKAGE.JSON', 'manifest.json', 'replaces the manifest on a case-insensitive filesystem'],
    ['Node_Modules/link', 'target', 'is in the dependencies directory on a case-insensitive filesystem'],
    ['link', 'NODE_MODULES/dep', 'names node_modules on a case-insensitive filesystem'],
    ['../../tmp/escape', 'target', 'is outside the package'],
    ['a/../../escape', 'target', 'is outside the package'],
    ['/tmp/escape', 'target', 'is outside the package'],
    ['a//link', 'target', 'is not a plain relative path'],
    ['a\\link', 'target', 'is not a plain relative path'],
    ['link', '', 'is empty'],
  ])('rejects the link at %s to "%s", which %s', (linkPath, target) => {
    expect(normalizeSymlinkTarget(linkPath, target)).toBeUndefined()
  })
})

describe('addFilesFromDir() with recordSymlinks', () => {
  itOnPosix('records a link inside the package instead of following it', () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()
    fs.writeFileSync(path.join(srcDir, 'index.js'), '// code')
    fs.mkdirSync(path.join(srcDir, 'lib'))
    fs.writeFileSync(path.join(srcDir, 'lib/index.js'), '// lib')
    fs.symlinkSync('index.js', path.join(srcDir, 'alias.js'))
    fs.symlinkSync('lib', path.join(srcDir, 'lib-link'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir, { recordSymlinks: true })

    expect(hasUnrecordedSymlinks).toBe(false)
    expect(Array.from(filesIndex.keys()).sort()).toStrictEqual(['alias.js', 'index.js', 'lib-link', 'lib/index.js'])
    expect(filesIndex.get('alias.js')!.mode).toBe(SYMLINK_MODE)
    expect(fs.readFileSync(filesIndex.get('alias.js')!.filePath, 'utf8')).toBe('index.js')
    expect(filesIndex.get('lib-link')!.mode).toBe(SYMLINK_MODE)
    expect(fs.readFileSync(filesIndex.get('lib-link')!.filePath, 'utf8')).toBe('lib')
  })

  itOnPosix('follows a link it cannot record and reports it', () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()
    fs.mkdirSync(path.join(srcDir, 'lib'))
    fs.writeFileSync(path.join(srcDir, 'lib/index.js'), '// lib')
    fs.mkdirSync(path.join(srcDir, 'sub'))
    fs.symlinkSync(path.join(srcDir, 'lib/index.js'), path.join(srcDir, 'sub/absolute.js'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir, { recordSymlinks: true })

    expect(hasUnrecordedSymlinks).toBe(true)
    expect(filesIndex.get('sub/absolute.js')!.mode).not.toBe(SYMLINK_MODE)
  })

  itOnPosix('ignores listed files traversing a directory symlink that escapes the package root', () => {
    const storeDir = temporaryDirectory()
    const outsideDir = temporaryDirectory()
    fs.writeFileSync(path.join(outsideDir, 'secret.txt'), 'SECRET')

    const srcDir = temporaryDirectory()
    fs.writeFileSync(path.join(srcDir, 'index.js'), '// code')
    fs.mkdirSync(path.join(srcDir, 'node_modules'))
    fs.symlinkSync(outsideDir, path.join(srcDir, 'node_modules/bundled-dep'))

    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir, {
      files: ['index.js', 'node_modules/bundled-dep/secret.txt'],
    })

    expect(filesIndex.has('index.js')).toBe(true)
    expect(filesIndex.has('node_modules/bundled-dep/secret.txt')).toBe(false)
  })
})

describe('restoring symlinks from side effects', () => {
  function createIndex (links: Record<string, string>, opts: { added?: Record<string, string>, base?: Record<string, string>, deleted?: string[] } = {}): { storeDir: string, pkgIndex: PackageFilesIndex } {
    const storeDir = temporaryDirectory()
    const cafs = createCafs(storeDir)
    const toFiles = (entries: Record<string, string>, mode: number): PackageFiles => new Map(Object.entries(entries).map(([name, content]) => {
      const buffer = Buffer.from(content)
      const { digest, checkedAt } = cafs.addFile(buffer, mode)
      return [name, { digest, checkedAt, mode, size: buffer.length }]
    }))
    const added = toFiles(opts.added ?? {}, 0o644)
    for (const [name, info] of toFiles(links, SYMLINK_MODE)) added.set(name, info)
    return {
      storeDir,
      pkgIndex: {
        algo: 'sha512',
        files: toFiles({ 'package.json': '{}', ...opts.base }, 0o644),
        sideEffects: new Map([['engine', { added, deleted: opts.deleted }]]),
      },
    }
  }

  const verifiers = [
    ['checkPkgFilesIntegrity', checkPkgFilesIntegrity],
    ['buildFileMapsFromIndex', buildFileMapsFromIndex],
  ] as const

  describe.each(verifiers)('%s()', (_name, verify) => {
    itOnPosix('separates the symlinks from the added files', () => {
      const { storeDir, pkgIndex } = createIndex({ 'bin/git-add': 'git', lib: 'src' }, {
        added: { 'bin/git': 'binary', 'src/index.js': '// code' },
        base: { 'lib/index.js': '// old' },
        deleted: ['lib/index.js'],
      })

      const sideEffectsMap = verify(storeDir, pkgIndex).sideEffectsMaps?.get('engine')

      expect(Array.from(sideEffectsMap!.added!.keys()).sort()).toStrictEqual(['bin/git', 'src/index.js'])
      expect(sideEffectsMap!.symlinks).toStrictEqual(new Map([['bin/git-add', 'git'], ['lib', 'src']]))
      expect(sideEffectsMap!.deleted).toStrictEqual(['lib/index.js'])
    })

    itOnPosix('drops an entry whose link target leaves the package', () => {
      const { storeDir, pkgIndex } = createIndex({ 'bin/git-add': '../../outside' })

      expect(verify(storeDir, pkgIndex).sideEffectsMaps).toBeUndefined()
    })

    itOnPosix('drops an entry that would write a file below a link', () => {
      const { storeDir, pkgIndex } = createIndex({ lib: 'src' }, { base: { 'lib/index.js': '// old' } })

      expect(verify(storeDir, pkgIndex).sideEffectsMaps).toBeUndefined()
    })

    itOnPosix('drops an entry that would create a link below another link', () => {
      const { storeDir, pkgIndex } = createIndex({ lib: 'src', 'lib/escape': '../x' })

      expect(verify(storeDir, pkgIndex).sideEffectsMaps).toBeUndefined()
    })
  })
})
