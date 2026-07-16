import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { createIndexedPkgImporter } from '../src/index.js'

test.each(['cold', 'store-hardlink', 'unrelated-hardlink'] as const)(
  'clone-or-copy makes a private writable projection from a %s target',
  (targetKind) => {
    const tmp = tempDir()
    const storeDir = path.join(tmp, 'store')
    const unrelatedDir = path.join(tmp, 'unrelated')
    const importTo = path.join(tmp, 'project', 'package')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.mkdirSync(unrelatedDir, { recursive: true })

    const storeIndex = path.join(storeDir, 'index.js')
    const storeManifest = path.join(storeDir, 'package.json')
    const unrelatedFile = path.join(unrelatedDir, 'index.js')
    fs.writeFileSync(storeIndex, 'store contents')
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')
    fs.writeFileSync(unrelatedFile, 'unrelated contents')
    fs.chmodSync(storeIndex, 0o555)
    fs.chmodSync(storeManifest, 0o444)
    fs.chmodSync(unrelatedFile, 0o444)

    const storeIndexDigest = digestFile(storeIndex)
    const storeManifestDigest = digestFile(storeManifest)
    const unrelatedDigest = digestFile(unrelatedFile)
    const storeIndexMode = fs.statSync(storeIndex).mode
    const storeManifestMode = fs.statSync(storeManifest).mode
    const unrelatedMode = fs.statSync(unrelatedFile).mode

    if (targetKind !== 'cold') {
      fs.mkdirSync(importTo, { recursive: true })
      fs.linkSync(
        targetKind === 'store-hardlink' ? storeIndex : unrelatedFile,
        path.join(importTo, 'index.js')
      )
      fs.linkSync(storeManifest, path.join(importTo, 'package.json'))
    }

    const importPackage = createIndexedPkgImporter('clone-or-copy')
    expect(importPackage(importTo, {
      filesMap: new Map([
        ['index.js', storeIndex],
        ['package.json', storeManifest],
      ]),
      force: false,
      resolvedFrom: 'store',
      safeToSkip: true,
    })).toMatch(/^(clone|copy)$/)

    const projectedIndex = path.join(importTo, 'index.js')
    const projectedManifest = path.join(importTo, 'package.json')
    expect(fs.readFileSync(projectedIndex, 'utf8')).toBe('store contents')
    if (process.platform !== 'win32') {
      expect(fs.statSync(projectedIndex).mode & 0o200).toBe(0o200)
      expect(fs.statSync(projectedIndex).mode & 0o777).toBe(0o755)
      expect(fs.statSync(projectedManifest).mode & 0o200).toBe(0o200)
      expect(sameFile(projectedIndex, storeIndex)).toBe(false)
      expect(sameFile(projectedIndex, unrelatedFile)).toBe(false)
      expect(sameFile(projectedManifest, storeManifest)).toBe(false)
    }
    expect(fs.statSync(projectedIndex).nlink).toBe(1)
    expect(fs.statSync(projectedManifest).nlink).toBe(1)

    fs.writeFileSync(projectedIndex, 'project mutation')
    expect(fs.readFileSync(projectedIndex, 'utf8')).toBe('project mutation')

    expect(digestFile(storeIndex)).toBe(storeIndexDigest)
    expect(digestFile(storeManifest)).toBe(storeManifestDigest)
    expect(digestFile(unrelatedFile)).toBe(unrelatedDigest)
    expect(fs.statSync(storeIndex).mode).toBe(storeIndexMode)
    expect(fs.statSync(storeManifest).mode).toBe(storeManifestMode)
    expect(fs.statSync(unrelatedFile).mode).toBe(unrelatedMode)
  }
)

test.each(['plain', 'trailing-separator'] as const)(
  'clone-or-copy reuses an existing private writable projection with a %s target path',
  (targetPathKind) => {
    const tmp = tempDir()
    const storeDir = path.join(tmp, 'store')
    const importTo = path.join(tmp, 'project', 'package')
    fs.mkdirSync(storeDir, { recursive: true })

    const storeIndex = path.join(storeDir, 'index.js')
    const storeManifest = path.join(storeDir, 'package.json')
    fs.writeFileSync(storeIndex, 'store contents')
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')
    fs.chmodSync(storeIndex, 0o444)
    fs.chmodSync(storeManifest, 0o444)
    const filesMap = new Map([
      ['package.json', storeManifest],
      ['index.js', storeIndex],
    ])
    const importPackage = createIndexedPkgImporter('clone-or-copy')
    const opts = { filesMap, force: false, resolvedFrom: 'store' as const }

    const targetPath = targetPathKind === 'trailing-separator' ? `${importTo}${path.sep}` : importTo
    expect(importPackage(targetPath, opts)).toMatch(/^(clone|copy)$/)
    const projectedIndex = path.join(importTo, 'index.js')
    const originalIdentity = fs.statSync(projectedIndex, { bigint: true })
    fs.writeFileSync(projectedIndex, 'built contents')
    fs.writeFileSync(path.join(importTo, 'generated.js'), 'generated contents')

    expect(importPackage(targetPath, opts)).toBeUndefined()
    const reusedIdentity = fs.statSync(projectedIndex, { bigint: true })
    expect(reusedIdentity.dev).toBe(originalIdentity.dev)
    expect(reusedIdentity.ino).toBe(originalIdentity.ino)
    expect(fs.readFileSync(projectedIndex, 'utf8')).toBe('built contents')
    expect(fs.readFileSync(path.join(importTo, 'generated.js'), 'utf8')).toBe('generated contents')
  }
)

test('clone-or-copy imports an already writable package file', () => {
  const tmp = tempDir()
  const storeFile = path.join(tmp, 'store', 'index.js')
  const importTo = path.join(tmp, 'project', 'package')
  fs.mkdirSync(path.dirname(storeFile), { recursive: true })
  fs.writeFileSync(storeFile, 'store contents')
  expect(fs.statSync(storeFile).mode & 0o200).toBe(0o200)

  const importPackage = createIndexedPkgImporter('clone-or-copy')
  expect(importPackage(importTo, {
    filesMap: new Map([['index.js', storeFile]]),
    force: false,
    resolvedFrom: 'store',
  })).toMatch(/^(clone|copy)$/)

  const projectedFile = path.join(importTo, 'index.js')
  fs.writeFileSync(projectedFile, 'project mutation')
  expect(fs.readFileSync(projectedFile, 'utf8')).toBe('project mutation')
  expect(fs.readFileSync(storeFile, 'utf8')).toBe('store contents')
})

test('clone-or-copy replaces a private projection that became read-only', () => {
  const tmp = tempDir()
  const storeDir = path.join(tmp, 'store')
  const importTo = path.join(tmp, 'project', 'package')
  fs.mkdirSync(storeDir, { recursive: true })
  const storeManifest = path.join(storeDir, 'package.json')
  fs.writeFileSync(storeManifest, '{"name":"fixture"}')
  fs.chmodSync(storeManifest, 0o444)

  const importPackage = createIndexedPkgImporter('clone-or-copy')
  const opts = {
    filesMap: new Map([['package.json', storeManifest]]),
    force: false,
    resolvedFrom: 'store' as const,
  }
  expect(importPackage(importTo, opts)).toMatch(/^(clone|copy)$/)
  const projectedManifest = path.join(importTo, 'package.json')
  fs.writeFileSync(projectedManifest, '{"name":"built"}')
  fs.chmodSync(projectedManifest, fs.statSync(projectedManifest).mode & ~0o200)
  const oldIdentity = fs.statSync(projectedManifest, { bigint: true })

  expect(importPackage(importTo, opts)).toMatch(/^(clone|copy)$/)
  expect(fs.readFileSync(projectedManifest, 'utf8')).toBe('{"name":"fixture"}')
  expect(fs.statSync(projectedManifest).mode & 0o200).toBe(0o200)
  if (process.platform !== 'win32') {
    const newIdentity = fs.statSync(projectedManifest, { bigint: true })
    expect(newIdentity.ino).not.toBe(oldIdentity.ino)
  }
  expect(fs.readFileSync(storeManifest, 'utf8')).toBe('{"name":"fixture"}')
  expect(fs.statSync(storeManifest).mode & 0o200).toBe(0)
})

test('clone-or-copy replaces a projection with a read-only intermediate directory', () => {
  const tmp = tempDir()
  const storeDir = path.join(tmp, 'store')
  const importTo = path.join(tmp, 'project', 'package')
  fs.mkdirSync(storeDir, { recursive: true })
  const storeManifest = path.join(storeDir, 'package.json')
  const storeIndex = path.join(storeDir, 'index.js')
  fs.writeFileSync(storeManifest, '{"name":"fixture"}')
  fs.writeFileSync(storeIndex, 'module.exports = true')

  const importPackage = createIndexedPkgImporter('clone-or-copy')
  const opts = {
    filesMap: new Map([
      ['package.json', storeManifest],
      ['lib/deep/index.js', storeIndex],
    ]),
    force: false,
    resolvedFrom: 'store' as const,
  }
  expect(importPackage(importTo, opts)).toMatch(/^(clone|copy)$/)
  const projectedManifest = path.join(importTo, 'package.json')
  const oldIdentity = fs.statSync(projectedManifest, { bigint: true })
  fs.chmodSync(path.join(importTo, 'lib'), 0o000)

  expect(importPackage(importTo, opts)).toMatch(/^(clone|copy)$/)
  expect(fs.statSync(path.join(importTo, 'lib')).mode & 0o200).toBe(0o200)
  if (process.platform !== 'win32') {
    expect(fs.statSync(projectedManifest, { bigint: true }).ino).not.toBe(oldIdentity.ino)
  }
})

function digestFile (filePath: string): string {
  return createHash('sha256').update(fs.readFileSync(filePath)).digest('hex')
}

function sameFile (left: string, right: string): boolean {
  const leftStat = fs.statSync(left, { bigint: true })
  const rightStat = fs.statSync(right, { bigint: true })
  return leftStat.dev === rightStat.dev && leftStat.ino === rightStat.ino
}
