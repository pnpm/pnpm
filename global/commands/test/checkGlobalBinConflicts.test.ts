import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { checkGlobalBinConflicts } from '@pnpm/global.commands'
import { type DependencyManifest } from '@pnpm/types'

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
  opts: { alias: string, bins: Record<string, string> }
): void {
  const installDir = makeTempDir()
  const depDir = path.join(installDir, 'node_modules', opts.alias)
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({ dependencies: { [opts.alias]: '1.0.0' } })
  )
  fs.writeFileSync(
    path.join(depDir, 'package.json'),
    JSON.stringify({ name: opts.alias, version: '1.0.0', bin: opts.bins })
  )
  // Create hash symlink so scanGlobalPackages discovers it
  fs.symlinkSync(installDir, path.join(globalDir, 'fakehash-' + opts.alias))
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
    ).resolves.toBeUndefined()
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
    ).resolves.toBeUndefined()
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
    ).resolves.toBeUndefined()
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
    ).resolves.toBeUndefined()
  })
})
