import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { listGlobalPackages } from '@pnpm/global.commands'
import { symlinkDirSync } from 'symlink-dir'

function makeTempDir (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-'))
}

function createGlobalPackage (
  globalDir: string,
  opts: { alias: string, name?: string, version?: string }
): void {
  const pkgName = opts.name ?? opts.alias
  const version = opts.version ?? '1.0.0'
  const installDir = makeTempDir()
  const depDir = path.join(installDir, 'node_modules', opts.alias)
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({ dependencies: { [opts.alias]: version } })
  )
  fs.writeFileSync(
    path.join(depDir, 'package.json'),
    JSON.stringify({ name: pkgName, version })
  )
  const safeAlias = opts.alias.replace(/\//g, '-')
  symlinkDirSync(installDir, path.join(globalDir, `fakehash-${safeAlias}`))
}

describe('listGlobalPackages', () => {
  it('outputs valid JSON when reportAs=json', async () => {
    const globalDir = makeTempDir()
    createGlobalPackage(globalDir, { alias: 'foo', version: '1.2.3' })
    createGlobalPackage(globalDir, { alias: 'bar', version: '4.5.6' })

    const out = await listGlobalPackages(globalDir, [], { reportAs: 'json' })
    const parsed = JSON.parse(out)
    expect(Array.isArray(parsed)).toBe(true)
    expect(parsed).toHaveLength(1)
    expect(parsed[0].path).toBe(globalDir)
    expect(parsed[0].private).toBe(true)
    expect(parsed[0].dependencies).toBeDefined()
    expect(parsed[0].dependencies.foo.version).toBe('1.2.3')
    expect(parsed[0].dependencies.foo.from).toBe('foo')
    expect(parsed[0].dependencies.bar.version).toBe('4.5.6')
  })

  it('outputs an empty-but-valid JSON array element when no packages installed', async () => {
    const globalDir = makeTempDir()

    const out = await listGlobalPackages(globalDir, [], { reportAs: 'json' })
    const parsed = JSON.parse(out)
    expect(Array.isArray(parsed)).toBe(true)
    expect(parsed).toHaveLength(1)
    expect(parsed[0].path).toBe(globalDir)
    expect(parsed[0].private).toBe(true)
    expect(parsed[0].dependencies).toEqual({})
  })

  it('outputs paths when reportAs=parseable', async () => {
    const globalDir = makeTempDir()
    createGlobalPackage(globalDir, { alias: 'foo', version: '1.2.3' })

    const out = await listGlobalPackages(globalDir, [], { reportAs: 'parseable' })
    const lines = out.split('\n')
    expect(lines[0]).toBe(globalDir)
    expect(lines.some((line) => line.endsWith(path.join('node_modules', 'foo')))).toBe(true)
  })

  it('outputs plain text by default', async () => {
    const globalDir = makeTempDir()
    createGlobalPackage(globalDir, { alias: 'foo', version: '1.2.3' })
    createGlobalPackage(globalDir, { alias: 'bar', version: '4.5.6' })

    const out = await listGlobalPackages(globalDir, [])
    expect(out).toContain('foo@1.2.3')
    expect(out).toContain('bar@4.5.6')
  })

  it('filters by parameters', async () => {
    const globalDir = makeTempDir()
    createGlobalPackage(globalDir, { alias: 'foo', version: '1.2.3' })
    createGlobalPackage(globalDir, { alias: 'bar', version: '4.5.6' })

    const out = await listGlobalPackages(globalDir, ['foo'], { reportAs: 'json' })
    const parsed = JSON.parse(out)
    expect(Object.keys(parsed[0].dependencies)).toEqual(['foo'])
  })
})
