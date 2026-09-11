// Exercises the `pn`, `pnpx`, and `pnx` alias bins: each hands over to the pnpm
// installed alongside it, whatever `PATH` names. They are `sh` scripts, so they
// run wherever `sh` does — on Windows the install script replaces them with
// hardlinks of the native binary, which infers the alias from its own name.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
/** Each alias and the argv it prepends: `pnpx` and `pnx` mean `pnpm dlx`. */
const ALIASES = [['pn', ''], ['pnpx', 'dlx '], ['pnx', 'dlx ']]
// No pnpm on it, so a `PATH` lookup finds nothing but the decoys the tests plant.
// `readlink` and `dirname` still have to be reachable.
const BARE_PATH = '/usr/bin:/bin'
const ARGS = ['add', 'foo']

const NO_SH = process.platform === 'win32' && 'Windows has no sh'

describe('alias bins', () => {
  for (const [alias, injected] of ALIASES) {
    const expected = `sibling: ${injected}${ARGS.join(' ')}\n`

    describe(alias, () => {
      it('parses as an sh script', { skip: NO_SH }, async () => {
        const result = await run('sh', ['-n', path.join(WRAPPER_DIR, alias)])
        assert.equal(result.status, 0, result.stderr)
      })

      // The native binary sits next to the alias, so it is reachable even where
      // the directory holding both is not on `PATH` — as `node_modules/.bin` is
      // not, outside a `pnpm run`.
      it('runs the pnpm beside it with no pnpm on PATH', { skip: NO_SH }, async () => {
        const { wrapperDir } = createFixture()

        const result = await run(path.join(wrapperDir, alias), ARGS, { PATH: BARE_PATH })
        assert.equal(result.status, 0, result.stderr)
        assert.equal(result.stdout, expected)
      })

      // Any other pnpm on `PATH` — a different major installed globally, or a
      // wrapper of one — would otherwise take over the call, and say nothing.
      it('ignores an unrelated pnpm earlier on PATH', { skip: NO_SH }, async () => {
        const { dir, wrapperDir } = createFixture()
        const decoyDir = path.join(dir, 'decoy')
        writeStub(path.join(decoyDir, 'pnpm'), 'decoy')

        const result = await run(path.join(wrapperDir, alias), ARGS, { PATH: `${decoyDir}:${BARE_PATH}` })
        assert.equal(result.status, 0, result.stderr)
        assert.equal(result.stdout, expected)
      })

      // A bin directory links the alias from this package while its own `pnpm`
      // comes from elsewhere, or is missing. The alias belongs to the package it
      // was linked from, so that is the pnpm it has to reach.
      it('resolves past a symlink to the package it was linked from', { skip: NO_SH }, async () => {
        const { dir, wrapperDir } = createFixture()
        const binDir = path.join(dir, 'node_modules', '.bin')
        writeStub(path.join(binDir, 'pnpm'), 'decoy')
        const link = path.join(binDir, alias)
        fs.symlinkSync(path.relative(binDir, path.join(wrapperDir, alias)), link)

        const result = await run(link, ARGS, { PATH: BARE_PATH })
        assert.equal(result.status, 0, result.stderr)
        assert.equal(result.stdout, expected)
      })

      // The walk's other branch: an absolute link replaces the path outright
      // rather than being joined onto the link's own directory.
      it('resolves past an absolute symlink', { skip: NO_SH }, async () => {
        const { dir, wrapperDir } = createFixture()
        const binDir = path.join(dir, 'node_modules', '.bin')
        writeStub(path.join(binDir, 'pnpm'), 'decoy')
        const link = path.join(binDir, alias)
        fs.symlinkSync(path.join(wrapperDir, alias), link)

        const result = await run(link, ARGS, { PATH: BARE_PATH })
        assert.equal(result.status, 0, result.stderr)
        assert.equal(result.stdout, expected)
      })

      // Two hops, mixing the branches, since the walk is a loop rather than a
      // single readlink.
      it('resolves a chain of symlinks', { skip: NO_SH }, async () => {
        const { dir, wrapperDir } = createFixture()
        const firstDir = path.join(dir, 'first')
        const secondDir = path.join(dir, 'second')
        fs.mkdirSync(firstDir, { recursive: true })
        fs.mkdirSync(secondDir, { recursive: true })
        writeStub(path.join(firstDir, 'pnpm'), 'decoy')
        writeStub(path.join(secondDir, 'pnpm'), 'decoy')
        fs.symlinkSync(path.join(wrapperDir, alias), path.join(secondDir, alias))
        fs.symlinkSync(path.relative(firstDir, path.join(secondDir, alias)), path.join(firstDir, alias))

        const result = await run(path.join(firstDir, alias), ARGS, { PATH: BARE_PATH })
        assert.equal(result.status, 0, result.stderr)
        assert.equal(result.stdout, expected)
      })
    })
  }

  // `readlink` is the one helper the walk still shells out to, so it runs through
  // `command -p`, which searches the system default PATH rather than the caller's.
  // A decoy here would otherwise get to report any target it liked, or just run.
  it('does not use a readlink from the caller\'s PATH', { skip: NO_SH }, async () => {
    const { dir, wrapperDir } = createFixture()
    const decoyDir = path.join(dir, 'decoy')
    writeStub(path.join(decoyDir, 'readlink'), 'HIJACKED')
    const binDir = path.join(dir, 'node_modules', '.bin')
    writeStub(path.join(binDir, 'pnpm'), 'decoy')
    const link = path.join(binDir, 'pnpx')
    fs.symlinkSync(path.relative(binDir, path.join(wrapperDir, 'pnpx')), link)

    const result = await run(link, ARGS, { PATH: `${decoyDir}:${BARE_PATH}` })
    assert.equal(result.status, 0, result.stderr)
    assert.equal(result.stdout, `sibling: dlx ${ARGS.join(' ')}\n`)
  })

  // What a script-less install leaves: `pnpm` is still the shebang-less
  // placeholder, which the kernel refuses and `exec` retries under a shell. The
  // alias has to reach it the same way a bin shim does.
  it('reaches the placeholder pnpm when the install script was skipped', { skip: NO_SH }, async () => {
    const { wrapperDir } = createFixture({ installBinary: false })

    const result = await run(path.join(wrapperDir, 'pnpx'), ARGS, { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
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
 * A wrapper directory holding the three alias bins and a stand-in `pnpm` that
 * reports the arguments it was handed — which is all the aliases have to get
 * right, and what the install script's copy of the native binary occupies. The
 * directory it sits in stands for the project the package was installed into.
 *
 * `installBinary: false` puts the real wrapper there instead, placeholder and
 * all, standing for an install whose scripts were skipped.
 */
function createFixture ({ installBinary = true } = {}) {
  // The space in the name is deliberate: the walk resolves directories with
  // `${self%/*}` and matches with `case`, neither of which field-splits, so every
  // test here doubles as coverage that a path with a space still resolves.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm alias '))
  after(() => fs.rmSync(dir, { force: true, recursive: true }))

  const wrapperDir = path.join(dir, 'node_modules', 'pnpm')
  const files = installBinary
    ? ALIASES.map(([alias]) => alias)
    : [...ALIASES.map(([alias]) => alias), 'pnpm', 'native-binary.mjs', 'bin/pnpm.mjs']
  for (const file of files) {
    fs.mkdirSync(path.dirname(path.join(wrapperDir, file)), { recursive: true })
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(wrapperDir, file))
    fs.chmodSync(path.join(wrapperDir, file), 0o755)
  }
  if (installBinary) {
    writeStub(path.join(wrapperDir, 'pnpm'), 'sibling')
  } else {
    fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '99.0.0' }))
  }

  return { dir, wrapperDir }
}

/**
 * An executable stand-in for `pnpm` at `file` that echoes `label` and its
 * arguments. chmod separately, since `writeFileSync`'s `mode` applies only when
 * it creates the file.
 */
function writeStub (file, label) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, `#!/bin/sh\necho "${label}: $*"\n`)
  fs.chmodSync(file, 0o755)
}
