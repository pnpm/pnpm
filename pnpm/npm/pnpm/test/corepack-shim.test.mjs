import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, resolveNativeBinary } from '../bin/pnpm.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const packageRoot = path.resolve(__dirname, '..')

const generatedTargets = [
  { codeTarget: 'win32-x64', ext: '.exe' },
  { codeTarget: 'win32-arm64', ext: '.exe' },
  { codeTarget: 'darwin-x64', ext: '' },
  { codeTarget: 'darwin-arm64', ext: '' },
  { codeTarget: 'linux-x64', ext: '' },
  { codeTarget: 'linux-arm64', ext: '' },
  { codeTarget: 'linux-x64-musl', ext: '' },
  { codeTarget: 'linux-arm64-musl', ext: '' },
]

test('published pnpm wrapper includes the Corepack entrypoints', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'))

  assert.equal(manifest.files.includes('bin/pnpm.mjs'), true)
  assert.equal(manifest.files.includes('bin/pnpx.mjs'), true)
})

test('generated exe wrapper copies the Corepack entrypoints', () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-generate-wrapper-'))
  const fixturePackageRoot = path.join(tempRoot, 'pnpm/npm/pnpm')
  const fixtureWrapperRoot = path.join(tempRoot, 'pnpm/npm/pnpm-exe')

  try {
    fs.mkdirSync(path.dirname(fixturePackageRoot), { recursive: true })
    fs.cpSync(packageRoot, fixturePackageRoot, {
      recursive: true,
      filter: (source) => !source.includes(`${path.sep}node_modules${path.sep}`),
    })
    for (const { codeTarget, ext } of generatedTargets) {
      const binaryPath = path.join(tempRoot, `pnpm-${codeTarget}${ext}`)
      fs.writeFileSync(binaryPath, 'native binary')
      fs.chmodSync(binaryPath, 0o755)
    }

    const result = spawnSync(process.execPath, [path.join(fixturePackageRoot, 'scripts/generate-packages.mjs')], {
      cwd: tempRoot,
      stdio: 'pipe',
    })
    assert.equal(result.status, 0, result.stderr.toString())

    for (const file of ['bin/pnpm.mjs', 'bin/pnpx.mjs']) {
      const sourcePath = path.join(fixturePackageRoot, file)
      const targetPath = path.join(fixtureWrapperRoot, file)
      assert.equal(fs.existsSync(targetPath), true)
      assert.equal(fs.statSync(targetPath).mode & 0o777, fs.statSync(sourcePath).mode & 0o777)
    }
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true })
  }
})

test('native binary candidates follow platform optional dependency packages', () => {
  assert.deepEqual(getBinCandidates({ platform: 'darwin', arch: 'arm64' }), ['@pnpm/exe.darwin-arm64/pnpm'])
  assert.deepEqual(getBinCandidates({ platform: 'linux', arch: 'x64', libc: 'glibc' }), [
    '@pnpm/exe.linux-x64/pnpm',
    '@pnpm/exe.linux-x64-musl/pnpm',
  ])
  assert.deepEqual(getBinCandidates({ platform: 'linux', arch: 'x64', libc: 'musl' }), [
    '@pnpm/exe.linux-x64-musl/pnpm',
    '@pnpm/exe.linux-x64/pnpm',
  ])
  assert.deepEqual(getBinCandidates({ platform: 'freebsd', arch: 'x64' }), [])
})

test('native binary resolution accepts the first installed candidate', () => {
  const resolved = resolveNativeBinary({
    candidates: ['@pnpm/exe.linux-x64/pnpm', '@pnpm/exe.linux-x64-musl/pnpm'],
    requireResolve: (target) => {
      if (target === '@pnpm/exe.linux-x64/pnpm') {
        throw Object.assign(new Error('not installed'), { code: 'MODULE_NOT_FOUND' })
      }
      return '/node_modules/@pnpm/exe.linux-x64-musl/pnpm'
    },
  })

  assert.equal(resolved, '/node_modules/@pnpm/exe.linux-x64-musl/pnpm')
})

test('native binary resolution does not mask invalid packages', () => {
  assert.throws(
    () => resolveNativeBinary({
      candidates: ['@pnpm/exe.linux-x64/pnpm', '@pnpm/exe.linux-x64-musl/pnpm'],
      requireResolve: (target) => {
        if (target === '@pnpm/exe.linux-x64/pnpm') {
          throw Object.assign(new Error('invalid package exports'), { code: 'ERR_PACKAGE_PATH_NOT_EXPORTED' })
        }
        return '/node_modules/@pnpm/exe.linux-x64-musl/pnpm'
      },
    }),
    { code: 'ERR_PACKAGE_PATH_NOT_EXPORTED' }
  )
})

test('Corepack entrypoints can run an installed native binary on POSIX', (t) => {
  if (process.platform === 'win32') {
    t.skip('Windows needs a real .exe fixture to execute the native binary path.')
    return
  }

  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    t.skip(`No native package fixture for ${process.platform}-${process.arch}.`)
    return
  }

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-corepack-shim-'))
  t.after(() => fs.rmSync(tempRoot, { recursive: true, force: true }))

  fs.cpSync(path.join(packageRoot, 'bin'), path.join(tempRoot, 'bin'), { recursive: true })

  const [candidate] = candidates
  const [scope, name, ...binarySegments] = candidate.split('/')
  const packageDir = path.join(tempRoot, 'node_modules', scope, name)
  const binaryPath = path.join(packageDir, ...binarySegments)
  fs.mkdirSync(packageDir, { recursive: true })
  fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({ name: `${scope}/${name}`, version: '0.0.0' }))
  fs.writeFileSync(
    binaryPath,
    '#!/usr/bin/env node\n' +
    'const fs = require("node:fs");\n' +
    'if (process.env.PNPM_SHIM_MODE === "status") process.exit(33);\n' +
    'if (process.env.PNPM_SHIM_MODE === "signal") process.kill(process.pid, "SIGTERM");\n' +
    'fs.writeFileSync(process.env.PNPM_SHIM_CAPTURE, JSON.stringify(process.argv.slice(2)));\n'
  )
  fs.chmodSync(binaryPath, 0o755)

  const capturePath = path.join(tempRoot, 'argv.json')
  const runEntrypoint = (entrypoint, args, env = {}) => spawnSync(
    process.execPath,
    [path.join(tempRoot, `bin/${entrypoint}.mjs`), ...args],
    {
      env: { ...process.env, PNPM_SHIM_CAPTURE: capturePath, ...env },
      stdio: 'pipe',
    }
  )

  const pnpmResult = runEntrypoint('pnpm', ['--version'])
  assert.equal(pnpmResult.status, 0, pnpmResult.stderr.toString())
  assert.deepEqual(JSON.parse(fs.readFileSync(capturePath, 'utf8')), ['--version'])

  const pnpxResult = runEntrypoint('pnpx', ['is-sorted'])
  assert.equal(pnpxResult.status, 0, pnpxResult.stderr.toString())
  assert.deepEqual(JSON.parse(fs.readFileSync(capturePath, 'utf8')), ['dlx', 'is-sorted'])

  assert.equal(runEntrypoint('pnpm', [], { PNPM_SHIM_MODE: 'status' }).status, 33)
  assert.equal(runEntrypoint('pnpx', [], { PNPM_SHIM_MODE: 'status' }).status, 33)

  assert.equal(runEntrypoint('pnpm', [], { PNPM_SHIM_MODE: 'signal' }).signal, 'SIGTERM')
  assert.equal(runEntrypoint('pnpx', [], { PNPM_SHIM_MODE: 'signal' }).signal, 'SIGTERM')
})
