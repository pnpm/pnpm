import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { checkGlobalBinConflicts } from '@pnpm/global.commands'
import type { DependencyManifest } from '@pnpm/types'
import { symlinkDirSync } from 'symlink-dir'

function makeTempDir (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-'))
}

/**
 * Creates a fake global directory structure that `scanGlobalPackages` can read.
 *
 * Layout:
 *   globalDir/
 *     <hash> -> installDir          (symlink)
 *   installDir/
 *     package.json                  { dependencies: { <alias>: "1.0.0" } }
 *     node_modules/<alias>/
 *       package.json                { name, version, bin }
 */
function createExistingGlobalPackage (
  globalDir: string,
  opts: { alias: string, name?: string, bins: Record<string, string> }
): void {
  const pkgName = opts.name ?? opts.alias
  const installDir = makeTempDir()
  const depDir = path.join(installDir, 'node_modules', opts.alias)
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({ dependencies: { [opts.alias]: '1.0.0' } })
  )
  fs.writeFileSync(
    path.join(depDir, 'package.json'),
    JSON.stringify({ name: pkgName, version: '1.0.0', bin: opts.bins })
  )
  // Create hash symlink so scanGlobalPackages discovers it
  const safeAlias = opts.alias.replace(/\//g, '-')
  symlinkDirSync(installDir, path.join(globalDir, `fakehash-${safeAlias}`))
}

function makeNewPkg (
  name: string,
  bins: Record<string, string>
): { manifest: DependencyManifest, location: string } {
  const dir = makeTempDir()
  fs.writeFileSync(
    path.join(dir, 'package.json'),
    JSON.stringify({ name, version: '1.0.0', bin: bins })
  )
  return {
    manifest: { name, version: '1.0.0', bin: bins } as DependencyManifest,
    location: dir,
  }
}

describe('checkGlobalBinConflicts', () => {
  it('allows install when no bins conflict', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    createExistingGlobalPackage(globalDir, {
      alias: 'typescript',
      bins: { tsc: './bin/tsc', tsserver: './bin/tsserver' },
    })

    // "eslint" has no overlapping bin names
    const newPkg = makeNewPkg('eslint', { eslint: './bin/eslint.js' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set())
  })

  it('throws on unrelated bin name conflict', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing package "foo" provides bin "shared-cmd"
    createExistingGlobalPackage(globalDir, {
      alias: 'foo',
      bins: { 'shared-cmd': './bin/cmd.js' },
    })
    // Create the bin file so the quick fs.existsSync check passes
    fs.writeFileSync(path.join(globalBinDir, 'shared-cmd'), '')

    // New package "bar" also provides "shared-cmd"
    const newPkg = makeNewPkg('bar', { 'shared-cmd': './bin/cmd.js' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).rejects.toThrow('would conflict')
  })

  it('allows override when package name matches bin name', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing "node" package provides "npm" bin
    createExistingGlobalPackage(globalDir, {
      alias: 'node',
      bins: { node: './bin/node', npm: './lib/npm-cli.js', npx: './lib/npx-cli.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'npm'), '')

    // New "npm" package provides "npm" bin — name matches, should win
    const newPkg = makeNewPkg('npm', { npm: './bin/npm-cli.js' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set())
  })

  it('allows override for npx when npm package is being installed (BIN_OWNER_OVERRIDES)', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing "node" package provides "npx" bin
    createExistingGlobalPackage(globalDir, {
      alias: 'node',
      bins: { node: './bin/node', npm: './lib/npm-cli.js', npx: './lib/npx-cli.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'npm'), '')
    fs.writeFileSync(path.join(globalBinDir, 'npx'), '')

    // New "npm" package provides both "npm" and "npx"
    const newPkg = makeNewPkg('npm', { npm: './bin/npm-cli.js', npx: './bin/npx-cli.js' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set())
  })

  it('still throws when an unowned bin conflicts even if another bin is owned', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing "other-pkg" provides "some-tool" bin
    createExistingGlobalPackage(globalDir, {
      alias: 'other-pkg',
      bins: { 'some-tool': './bin/tool.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'some-tool'), '')
    fs.writeFileSync(path.join(globalBinDir, 'my-cli'), '')

    // New "my-cli" owns "my-cli" but also ships "some-tool"
    const newPkg = makeNewPkg('my-cli', {
      'my-cli': './bin/cli.js',
      'some-tool': './bin/tool.js',
    })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).rejects.toThrow('would conflict')
  })

  it('skips packages matched by shouldSkip (same-package upgrade)', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    createExistingGlobalPackage(globalDir, {
      alias: 'typescript',
      bins: { tsc: './bin/tsc' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'tsc'), '')

    const newPkg = makeNewPkg('typescript', { tsc: './bin/tsc' })

    // shouldSkip returns true for the existing typescript package
    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: (pkg) => 'typescript' in pkg.dependencies,
      })
    ).resolves.toEqual(new Set())
  })

  it('allows override when one of multiple new packages owns the bin', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing "old-pkg" provides "foo" bin
    createExistingGlobalPackage(globalDir, {
      alias: 'old-pkg',
      bins: { foo: './bin/foo.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'foo'), '')

    // Two new packages both provide "foo"; "foo" owns the bin by name
    const newPkgA = makeNewPkg('bar', { foo: './bin/foo.js' })
    const newPkgB = makeNewPkg('foo', { foo: './bin/foo.js' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkgA, newPkgB],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set())
  })

  it('uses manifest.name instead of alias for existing package ownership', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing package has alias "my-npm" but its real name is "npm"
    createExistingGlobalPackage(globalDir, {
      alias: 'my-npm',
      name: 'npm',
      bins: { npm: './bin/npm-cli.js', npx: './bin/npx-cli.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'npm'), '')
    fs.writeFileSync(path.join(globalBinDir, 'npx'), '')

    // New "node" package provides "npm" and "npx" — the existing package
    // owns them (real name "npm"), so they should be skipped.
    const newPkg = makeNewPkg('node', {
      node: './bin/node',
      npm: './lib/npm-cli.js',
      npx: './lib/npx-cli.js',
    })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set(['npm', 'npx']))
  })

  it('throws when @pnpm/exe conflicts with existing pnpm package', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    createExistingGlobalPackage(globalDir, {
      alias: 'pnpm',
      bins: { pnpm: './bin/pnpm.mjs', pnpx: './bin/pnpx.mjs' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'pnpm'), '')
    fs.writeFileSync(path.join(globalBinDir, 'pnpx'), '')

    const newPkg = makeNewPkg('@pnpm/exe', { pnpm: './pnpm', pnpx: './pnpx' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).rejects.toThrow('would conflict')
  })

  it('throws when pnpm conflicts with existing @pnpm/exe package', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    createExistingGlobalPackage(globalDir, {
      alias: '@pnpm/exe',
      name: '@pnpm/exe',
      bins: { pnpm: './pnpm', pnpx: './pnpx' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'pnpm'), '')
    fs.writeFileSync(path.join(globalBinDir, 'pnpx'), '')

    const newPkg = makeNewPkg('pnpm', { pnpm: './bin/pnpm.mjs', pnpx: './bin/pnpx.mjs' })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).rejects.toThrow('would conflict')
  })

  it('returns bins to skip when existing package owns conflicting bins', async () => {
    const globalDir = makeTempDir()
    const globalBinDir = makeTempDir()

    // Existing "npm" package provides "npm" and "npx" bins
    createExistingGlobalPackage(globalDir, {
      alias: 'npm',
      bins: { npm: './bin/npm-cli.js', npx: './bin/npx-cli.js' },
    })
    fs.writeFileSync(path.join(globalBinDir, 'npm'), '')
    fs.writeFileSync(path.join(globalBinDir, 'npx'), '')

    // New "node" package provides "node", "npm", and "npx"
    // "node" doesn't own "npm" or "npx", but the existing "npm" package does,
    // so those bins should be skipped rather than causing an error.
    const newPkg = makeNewPkg('node', {
      node: './bin/node',
      npm: './lib/npm-cli.js',
      npx: './lib/npx-cli.js',
    })

    await expect(
      checkGlobalBinConflicts({
        globalDir,
        globalBinDir,
        newPkgs: [newPkg],
        shouldSkip: () => false,
      })
    ).resolves.toEqual(new Set(['npm', 'npx']))
  })
})
