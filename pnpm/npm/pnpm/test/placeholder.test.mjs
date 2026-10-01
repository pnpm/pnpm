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
const FAKE_BINARY_OUTPUT = /^installed: --version\n$/

const IS_UNIX = process.platform !== 'win32'
describe('Node fallback bin', () => {
  it('has a Node shebang for hosts that cannot run shell placeholders', () => {
    assert.match(fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'), /^#!\/usr\/bin\/env node\n/)
  })

  it('runs the installed platform binary through the Node entry point', async () => {
    const fixture = createFixture()
    const result = await run(process.execPath, [fixture.placeholder, '--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.equal(result.stdout, IS_UNIX ? 'installed: --version\n' : `${process.version}${os.EOL}`)
  })

  it('runs directly through its shebang on Unix', { skip: !IS_UNIX }, async () => {
    const fixture = createFixture()
    const result = await run(fixture.placeholder, ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
  })

  it('resolves a symlink to its own entry point', { skip: !IS_UNIX }, async () => {
    const fixture = createFixture()
    const binDir = path.join(fixture.dir, 'linked', '.bin')
    fs.mkdirSync(binDir, { recursive: true })
    const link = path.join(binDir, 'pnpm')
    fs.symlinkSync(path.relative(binDir, fixture.placeholder), link)
    const result = await run(process.execPath, [link, '--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
  })

  it('hands over to the entry point when no platform package is installed', async () => {
    const fixture = createFixture({ installPlatformPackage: false })
    const result = await run(process.execPath, [fixture.placeholder, '--version'], { env: { COREPACK_ENABLE_NETWORK: '0' } })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
  })

  it('does not run a platform package from an ancestor node_modules', async () => {
    const fixture = createFixture({ installPlatformPackage: false, nestedUnder: ['node_modules', 'tool', 'node_modules'] })
    writePlatformPackage(path.join(fixture.dir, 'node_modules'))
    const result = await run(process.execPath, [fixture.placeholder, '--version'], { env: { COREPACK_ENABLE_NETWORK: '0' } })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
    assert.doesNotMatch(result.stdout, FAKE_BINARY_OUTPUT)
  })
})

/**
 * Spawn `command` with `args`. Resolves once the child has exited, with its
 * exit status and decoded output; rejects only if it could not be spawned.
 *
 * @param {string} command Executable to spawn, as an absolute path or a name on `PATH`.
 * @param {string[]} args Arguments to pass to it.
 * @param {{env?: Record<string, string>, cwd?: string}} [options] `env` is
 *   layered over `process.env`, which is inherited unchanged when omitted;
 *   `cwd` defaults to the current directory.
 * @returns {Promise<{status: number | null, stdout: string, stderr: string}>}
 *   `status` is null when a signal ended the child.
 */
function run (command, args, { env, cwd } = {}) {
  const child = spawn(command, args, { cwd, env: { ...process.env, ...env } })

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
  fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '99.0.0', type: 'module' }))

  if (installPlatformPackage) {
    writePlatformPackage(path.join(wrapperDir, 'node_modules'))
  }

  return { dir, placeholder: path.join(wrapperDir, 'pnpm'), entryPoint: path.join(wrapperDir, 'bin', 'pnpm.mjs') }
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
