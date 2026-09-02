// Exercises the `pnpm` placeholder bin the way a script-less install leaves it:
// the install script never replaced it with the native binary, and it is what
// the package manager's bin shim runs — through Node.js, since its shebang
// names it.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = ['pnpm', 'native-binary.mjs', 'bin/pnpm.mjs']
// A stand-in for the native binary. On Windows it is a copy of node itself,
// which is why the tests ask for `--version`; elsewhere a script that echoes.
const FAKE_BINARY_OUTPUT = process.platform === 'win32' ? /^v\d+/ : /^installed: --version\n$/

const IS_UNIX = process.platform !== 'win32'

describe('placeholder bin', () => {
  // What every bin shim made from this file resolves it to.
  it('names node in its shebang', () => {
    assert.match(fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'), /^#!\/usr\/bin\/env node\n/)
  })

  it('runs the installed native binary through Node.js', async () => {
    const fixture = createFixture()

    const result = await run(process.execPath, [fixture.placeholder, '--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
    // Not a terminal, so no notice.
    assert.equal(result.stderr, '')
    assert.equal(fs.readFileSync(fixture.placeholder, 'utf8'), fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'))
  })

  // npm links `node_modules/.bin/pnpm` straight to the file and the kernel
  // reads the shebang; Windows has neither, so this is Unix-only.
  it('runs from a symlink to itself', { skip: !IS_UNIX && 'Windows bins are shims, not symlinks' }, async () => {
    const fixture = createFixture()
    const binDir = path.join(fixture.dir, 'node_modules', '.bin')
    fs.mkdirSync(binDir, { recursive: true })
    const link = path.join(binDir, 'pnpm')
    fs.symlinkSync(path.relative(binDir, fixture.placeholder), link)

    const result = await run(link, ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
  })

  it('hands over to the entry point when no platform package is installed', async () => {
    const fixture = createFixture({ installPlatformPackage: false })

    const result = await run(process.execPath, [fixture.placeholder, '--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
  })

  // A project the wrapper sits under can hold anything under the platform
  // package's name; only what was installed with the wrapper is its binary.
  it('does not run a platform package from an ancestor node_modules', async () => {
    const fixture = createFixture({ installPlatformPackage: false, nestedUnder: ['node_modules', 'tool', 'node_modules'] })
    writePlatformPackage(path.join(fixture.dir, 'node_modules'))

    const result = await run(process.execPath, [fixture.placeholder, '--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
    assert.doesNotMatch(result.stdout, FAKE_BINARY_OUTPUT)
  })
})

/**
 * Spawn `command` with `args`, `env` overriding the inherited environment.
 * Resolves once the child has exited, with its exit status and decoded output;
 * rejects only if it could not be spawned.
 *
 * @returns {Promise<{status: number | null, stdout: string, stderr: string}>}
 */
function run (command, args, env) {
  const child = spawn(command, args, { env: { ...process.env, ...env } })

  let stdout = ''
  let stderr = ''
  child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk })
  child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk })

  return new Promise((resolve, reject) => {
    child.on('error', reject)
    child.on('close', (status) => { resolve({ status, stdout, stderr }) })
  })
}

/**
 * A wrapper directory as a script-less install leaves it: the placeholder still
 * in place, and — unless told otherwise — the platform package that carries the
 * binary installed in the wrapper's own `node_modules`, since only the scripts
 * were skipped. `nestedUnder` places the wrapper that many directories below
 * the fixture root, which then stands for a project the wrapper sits under.
 */
function createFixture ({ installPlatformPackage = true, nestedUnder = [] } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-placeholder-'))
  after(() => fs.rmSync(dir, { force: true, recursive: true }))
  const wrapperDir = path.join(dir, ...nestedUnder, nestedUnder.length > 0 ? 'pnpm' : '')

  for (const file of WRAPPER_FILES) {
    fs.mkdirSync(path.dirname(path.join(wrapperDir, file)), { recursive: true })
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(wrapperDir, file))
  }
  fs.chmodSync(path.join(wrapperDir, 'pnpm'), 0o755)
  // `type` is what makes Node.js read the extensionless placeholder as ESM.
  fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '99.0.0', type: 'module' }))

  if (installPlatformPackage) {
    writePlatformPackage(path.join(wrapperDir, 'node_modules'))
  }

  return { dir, placeholder: path.join(wrapperDir, 'pnpm') }
}

/**
 * Create the host's `@pnpm/exe.<target>` package under `modulesDir` (created if
 * missing): a manifest and the stand-in binary — an executable `sh` script on
 * Unix, a copy of the running node on Windows. Filesystem errors propagate.
 */
function writePlatformPackage (modulesDir) {
  const { packageName, binFile } = splitBinSpecifier(getBinCandidates()[0])
  const packageDir = path.join(modulesDir, packageName)
  fs.mkdirSync(packageDir, { recursive: true })
  fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({ name: packageName, version: '99.0.0' }))
  if (IS_UNIX) {
    fs.writeFileSync(path.join(packageDir, binFile), '#!/bin/sh\necho "installed: $*"\n', { mode: 0o755 })
  } else {
    fs.copyFileSync(process.execPath, path.join(packageDir, binFile))
  }
}
