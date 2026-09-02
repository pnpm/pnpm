// Exercises the `pnpm` placeholder bin the way a script-less install leaves it:
// the install script never replaced it with the native binary, and it is what
// the package manager's bin shim (or npm's symlink) runs. The placeholder is a
// shebang-less `sh` script, so the tests that run it are Unix-only; what holds
// it to that shape is asserted everywhere.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = ['pnpm', 'native-binary.mjs', 'bin/pnpm.mjs']
// A stand-in for the native binary: reports how it was called, and whether the
// placeholder's own marker leaked into its environment.
const FAKE_BINARY = `#!/bin/sh
echo "installed: $*"
echo "marker: \${PNPM_WRAPPER_PLACEHOLDER-unset}"
`

const RUNS_SH = process.platform === 'win32' && 'the placeholder is a sh script, which Windows cannot run'

describe('placeholder bin', () => {
  // npm generates the Windows shim from this file before the install script
  // rewrites it; a `#!` line would make that shim run an interpreter over the
  // native binary placed here afterwards.
  it('carries no shebang', () => {
    assert.doesNotMatch(fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'), /^#!/)
  })

  it('runs the installed native binary, then takes its place', { skip: RUNS_SH }, async () => {
    const fixture = createFixture()

    const first = await runThroughShell(fixture.placeholder, ['--version'])
    assert.equal(first.status, 0, first.stderr)
    assert.match(first.stdout, /^installed: --version\nmarker: unset\n$/)
    assert.equal(fs.readFileSync(fixture.placeholder, 'utf8'), FAKE_BINARY)

    const second = await runThroughShell(fixture.placeholder, ['install'])
    assert.equal(second.status, 0, second.stderr)
    assert.match(second.stdout, /^installed: install\n/)
  })

  // npm links `node_modules/.bin/pnpm` straight to the file, so the placeholder
  // is entered under the link's name and has to find the wrapper from there.
  it('finds the wrapper through a symlink to itself', { skip: RUNS_SH }, async () => {
    const fixture = createFixture()
    const binDir = path.join(fixture.dir, 'node_modules', '.bin')
    fs.mkdirSync(binDir, { recursive: true })
    const link = path.join(binDir, 'pnpm')
    fs.symlinkSync(path.relative(binDir, fixture.placeholder), link)

    const result = await runThroughShell(link, ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /^installed: --version\n/)
  })

  it('keeps running when it cannot be replaced', { skip: RUNS_SH || (process.getuid?.() === 0 && 'root can write anywhere') }, async () => {
    const fixture = createFixture()
    const placeholderBefore = fs.readFileSync(fixture.placeholder, 'utf8')
    fs.chmodSync(fixture.dir, 0o555)
    let result
    try {
      result = await runThroughShell(fixture.placeholder, ['--version'])
    } finally {
      fs.chmodSync(fixture.dir, 0o755)
    }
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /^installed: --version\n/)
    assert.equal(fs.readFileSync(fixture.placeholder, 'utf8'), placeholderBefore)
  })

  it('hands over to the entry point when no platform package is installed', { skip: RUNS_SH }, async () => {
    const fixture = createFixture({ installPlatformPackage: false })

    const result = await runThroughShell(fixture.placeholder, ['--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
    assert.equal(fs.readFileSync(fixture.placeholder, 'utf8'), fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'))
  })

  it('says what is missing when there is no Node.js to hand over to', { skip: RUNS_SH }, async () => {
    const fixture = createFixture()
    // Only what the placeholder itself runs, so `node` is not found.
    const pathDir = path.join(fixture.dir, 'path')
    fs.mkdirSync(pathDir)
    for (const tool of ['readlink', 'dirname']) {
      fs.symlinkSync(execFileSync('sh', ['-c', `command -v ${tool}`], { encoding: 'utf8' }).trim(), path.join(pathDir, tool))
    }

    const result = await runThroughShell(fixture.placeholder, ['--version'], { PATH: pathDir })
    assert.equal(result.status, 127)
    assert.match(result.stderr, /install script that puts it in place did not run/)
    assert.match(result.stderr, /Node\.js, which could stand in for it, is not on PATH/)
  })
})

/**
 * Run `file` the way bin shims and shells do: `exec` from a POSIX shell, which
 * falls back to running a `#!`-less file as a `sh` script.
 */
function runThroughShell (file, args, env) {
  const child = spawn('/bin/sh', ['-c', 'exec "$0" "$@"', file, ...args], {
    env: { ...process.env, ...env },
  })

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
 * binary installed next to it, since only the scripts were skipped.
 */
function createFixture ({ installPlatformPackage = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-placeholder-'))
  after(() => fs.rmSync(dir, { force: true, recursive: true }))

  for (const file of WRAPPER_FILES) {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true })
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(dir, file))
  }
  fs.chmodSync(path.join(dir, 'pnpm'), 0o755)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '99.0.0' }))

  if (installPlatformPackage) {
    const { packageName, binFile } = splitBinSpecifier(getBinCandidates()[0])
    const packageDir = path.join(dir, 'node_modules', packageName)
    fs.mkdirSync(packageDir, { recursive: true })
    fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({ name: packageName, version: '99.0.0' }))
    fs.writeFileSync(path.join(packageDir, binFile), FAKE_BINARY, { mode: 0o755 })
  }

  return { dir, placeholder: path.join(dir, 'pnpm') }
}
