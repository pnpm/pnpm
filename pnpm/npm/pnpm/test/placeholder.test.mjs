// Exercises the `pnpm` placeholder bin the way a script-less install leaves it:
// the install script never replaced it with the native binary, so it is what
// runs — as an `sh` script, since it carries no shebang for the kernel to read.
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
// A shebang-less file runs only where something retries it under a shell after
// the kernel answers ENOEXEC. Every shell does, and so does glibc's `execvp` —
// but Apple's libc does not, and Windows has neither. So a shim or a shell
// reaches the placeholder wherever `sh` exists, while a bare `spawn` of it
// reaches it on Linux alone.
const HAS_A_SHELL = !IS_UNIX && 'Windows has no sh'
const SPAWNS_A_SHEBANGLESS_FILE = process.platform !== 'linux' &&
  `${process.platform} does not retry a shebang-less file under a shell`

describe('placeholder bin', () => {
  // The constraint the whole file exists under: a bin shim generated from a
  // shebang records that interpreter, and pnpm 11 generates the shim before it
  // puts the native binary at this path.
  it('carries no shebang', () => {
    assert.doesNotMatch(fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'), /^#!/)
  })

  it('parses as an sh script', { skip: HAS_A_SHELL }, async () => {
    const result = await run('sh', ['-n', path.join(WRAPPER_DIR, 'pnpm')])
    assert.equal(result.status, 0, result.stderr)
  })

  // Spawned with no shell in between, which only Linux resolves.
  it('runs the installed native binary', { skip: SPAWNS_A_SHEBANGLESS_FILE }, async () => {
    const fixture = createFixture()

    const result = await run(fixture.placeholder, ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
    // Not a terminal, so no notice.
    assert.equal(result.stderr, '')
    assert.equal(fs.readFileSync(fixture.placeholder, 'utf8'), fs.readFileSync(path.join(WRAPPER_DIR, 'pnpm'), 'utf8'))
  })

  // How pnpm links a bin when it symlinks executables, which is what pnpm 10
  // does for the version store it delegates a `packageManager` pin to. Started
  // from a shell, as a user's `pnpm` is, so the symlink chain `$0` walks is
  // exercised on macOS too.
  it('runs from a symlink to itself', { skip: HAS_A_SHELL }, async () => {
    const fixture = createFixture()
    const binDir = path.join(fixture.dir, 'node_modules', '.bin')
    fs.mkdirSync(binDir, { recursive: true })
    const link = path.join(binDir, 'pnpm')
    fs.symlinkSync(path.relative(binDir, fixture.placeholder), link)

    const result = await run('sh', [link, '--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
  })

  // What a bin linker writes for a target with no shebang, and the shape pnpm 11
  // leaves behind: an `exec` of the file itself, so the same shim keeps working
  // once the native binary takes its place.
  it('runs from a bin shim that execs it', { skip: HAS_A_SHELL }, async () => {
    const fixture = createFixture()
    const binDir = path.join(fixture.dir, 'node_modules', '.bin')
    fs.mkdirSync(binDir, { recursive: true })
    const shim = path.join(binDir, 'pnpm')
    fs.writeFileSync(shim, `#!/bin/sh\nbasedir=$(dirname "$0")\nexec "$basedir/${path.relative(binDir, fixture.placeholder)}" "$@"\n`, { mode: 0o755 })

    const result = await run(shim, ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, FAKE_BINARY_OUTPUT)
  })

  it('hands over to the entry point when no platform package is installed', { skip: HAS_A_SHELL }, async () => {
    const fixture = createFixture({ installPlatformPackage: false })

    const result = await run('sh', [fixture.placeholder, '--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
  })

  // A project the wrapper sits under can hold anything under the platform
  // package's name; only what was installed with the wrapper is its binary.
  // Launched through the entry point, whose rule this is, so Windows is covered
  // too — the placeholder cannot run there.
  it('does not run a platform package from an ancestor node_modules', async () => {
    const fixture = createFixture({ installPlatformPackage: false, nestedUnder: ['node_modules', 'tool', 'node_modules'] })
    writePlatformPackage(path.join(fixture.dir, 'node_modules'))

    const result = await run(process.execPath, [fixture.entryPoint, '--version'], { COREPACK_ENABLE_NETWORK: '0' })
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
 * @param {string} command Executable to spawn, as an absolute path or a name on `PATH`.
 * @param {string[]} args Arguments to pass to it.
 * @param {Record<string, string>} [env] Variables layered over `process.env`; the
 *   environment is inherited unchanged when omitted.
 * @returns {Promise<{status: number | null, stdout: string, stderr: string}>}
 *   `status` is null when a signal ended the child.
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
  fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '99.0.0' }))

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
