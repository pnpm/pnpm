import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, test } from '@jest/globals'
import { resolvePnpmExecPath, resolvePnpmSelfCommand } from '@pnpm/cli.meta'

import { findPnpmEntryScript } from '../src/selfEntry.js'

const tempDirs: string[] = []

afterEach(() => {
  while (tempDirs.length > 0) {
    fs.rmSync(tempDirs.pop()!, { recursive: true, force: true })
  }
})

describe('findPnpmEntryScript()', () => {
  test.each([
    ['pnpm', 'pnpm.mjs'],
    ['pn', 'pnpm.mjs'],
    ['pnpm', 'pnpm.cjs'],
  ])('accepts node_modules/.bin/%s linked to bin/%s of the running pnpm', (binName, scriptName) => {
    const { bundle, binLink } = installPnpm({ binName, scriptName })

    expect(findPnpmEntryScript(binLink, bundle)).toBe(binLink)
  })

  test('accepts the entry script of the running pnpm reached without a link', () => {
    const { bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    const entryScript = path.join(path.dirname(path.dirname(bundle)), 'bin', 'pnpm.mjs')

    expect(findPnpmEntryScript(entryScript, bundle)).toBe(entryScript)
  })

  test('accepts a bundle that is run directly, with no package manifest above it', () => {
    const bundle = path.join(makeTempDir(), 'pnpm.mjs')
    fs.writeFileSync(bundle, '')

    expect(findPnpmEntryScript(bundle, bundle)).toBe(bundle)
  })

  test('accepts a bundle copied into another project and run directly', () => {
    const root = makeTempDir()
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'consuming-project' }))
    fs.mkdirSync(path.join(root, 'tools'))
    const bundle = path.join(root, 'tools', 'pnpm.mjs')
    fs.writeFileSync(bundle, '')

    expect(findPnpmEntryScript(bundle, bundle)).toBe(bundle)
  })

  test('rejects a bin of another package installed next to the running pnpm', () => {
    const { root, bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    const otherPkgDir = path.join(root, 'node_modules', 'not-pnpm')
    fs.mkdirSync(otherPkgDir)
    fs.writeFileSync(path.join(otherPkgDir, 'package.json'), JSON.stringify({ name: 'not-pnpm', bin: { pn: 'cli.js' } }))
    fs.writeFileSync(path.join(otherPkgDir, 'cli.js'), '')
    const binLink = path.join(root, 'node_modules', '.bin', 'pn')
    fs.symlinkSync(path.join(otherPkgDir, 'cli.js'), binLink)

    expect(findPnpmEntryScript(binLink, bundle)).toBeUndefined()
  })

  test('rejects a script of the consuming project that only carries a pnpm name', () => {
    const { root, bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    fs.mkdirSync(path.join(root, 'scripts'))
    const entryScript = path.join(root, 'scripts', 'pnpm.mjs')
    fs.writeFileSync(entryScript, '')

    expect(findPnpmEntryScript(entryScript, bundle)).toBeUndefined()
  })

  test('rejects pnpx, although it belongs to the running pnpm', () => {
    const { bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    const pnpx = path.join(path.dirname(path.dirname(bundle)), 'bin', 'pnpx.mjs')
    fs.writeFileSync(pnpx, '')

    expect(findPnpmEntryScript(pnpx, bundle)).toBeUndefined()
  })

  test('rejects the entry script of a host that merely imports pnpm\'s packages', () => {
    const { root, bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    const jestBin = path.join(root, 'node_modules', 'jest', 'bin', 'jest.js')
    fs.mkdirSync(path.dirname(jestBin), { recursive: true })
    fs.writeFileSync(jestBin, '')

    expect(findPnpmEntryScript(jestBin, bundle)).toBeUndefined()
  })

  test('rejects an entry script whose link target is gone', () => {
    const { root, bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })
    const entryScript = path.join(root, 'pnpm.mjs')
    fs.symlinkSync(path.join(root, 'missing.mjs'), entryScript)

    expect(findPnpmEntryScript(entryScript, bundle)).toBeUndefined()
  })

  test('rejects an entry script next to the running module when neither has a package manifest', () => {
    const root = makeTempDir()
    const bundle = path.join(root, 'dist', 'pnpm.mjs')
    const entryScript = path.join(root, 'bin', 'pnpm.mjs')
    fs.mkdirSync(path.dirname(bundle))
    fs.mkdirSync(path.dirname(entryScript))
    fs.writeFileSync(bundle, '')
    fs.writeFileSync(entryScript, '')

    expect(findPnpmEntryScript(entryScript, bundle)).toBeUndefined()
  })

  test('rejects a missing entry script', () => {
    const { bundle } = installPnpm({ binName: 'pnpm', scriptName: 'pnpm.mjs' })

    expect(findPnpmEntryScript(undefined, bundle)).toBeUndefined()
  })
})

// Jest's own entry script is what `process.argv[1]` holds here, so these pin
// the fallback a host that merely imports pnpm's packages gets.
test('resolvePnpmSelfCommand() falls back to the pnpm on PATH outside pnpm', () => {
  expect(resolvePnpmSelfCommand()).toStrictEqual(['pnpm'])
})

test('resolvePnpmExecPath() finds no pnpm outside pnpm', () => {
  expect(resolvePnpmExecPath()).toBeUndefined()
})

/**
 * Lays out an npm-installed pnpm: the package with its `bin` entry and the
 * bundle it loads, and the `.bin` link to that entry, which is what
 * `process.argv[1]` would be.
 */
function installPnpm ({ binName, scriptName }: { binName: string, scriptName: string }): { root: string, bundle: string, binLink: string } {
  const root = makeTempDir()
  const pkgDir = path.join(root, 'node_modules', 'pnpm')
  fs.mkdirSync(path.join(pkgDir, 'bin'), { recursive: true })
  fs.mkdirSync(path.join(pkgDir, 'dist'))
  fs.mkdirSync(path.join(root, 'node_modules', '.bin'))
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'consuming-project' }))
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pnpm' }))
  const script = path.join(pkgDir, 'bin', scriptName)
  const bundle = path.join(pkgDir, 'dist', 'pnpm.mjs')
  fs.writeFileSync(script, '')
  fs.writeFileSync(bundle, '')
  const binLink = path.join(root, 'node_modules', '.bin', binName)
  fs.symlinkSync(script, binLink)
  return { root, bundle, binLink }
}

function makeTempDir (): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-meta-'))
  tempDirs.push(dir)
  return dir
}
