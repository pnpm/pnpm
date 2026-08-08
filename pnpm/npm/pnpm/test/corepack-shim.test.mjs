import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, resolveNativeBinary } from '../bin/pnpm.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const packageRoot = path.resolve(__dirname, '..')

test('published pnpm wrapper includes the Corepack entrypoints', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'))

  assert.equal(manifest.files.includes('bin/pnpm.mjs'), true)
  assert.equal(manifest.files.includes('bin/pnpx.mjs'), true)
})

test('generated exe wrapper copies the Corepack entrypoints', () => {
  const generatePackages = fs.readFileSync(path.join(packageRoot, 'scripts/generate-packages.mjs'), 'utf8')

  assert.match(generatePackages, /"bin\/pnpm\.mjs"/)
  assert.match(generatePackages, /"bin\/pnpx\.mjs"/)
  assert.match(generatePackages, /fs\.mkdirSync\(dirname\(target\), \{ recursive: true \}\)/)
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
        throw new Error('not installed')
      }
      return '/node_modules/@pnpm/exe.linux-x64-musl/pnpm'
    },
  })

  assert.equal(resolved, '/node_modules/@pnpm/exe.linux-x64-musl/pnpm')
})

test('Corepack entrypoints can run an installed native binary on POSIX', async (t) => {
  if (process.platform === 'win32') {
    t.skip('Windows needs a real .exe fixture to execute the native binary path.')
    return
  }

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-corepack-shim-'))
  t.after(() => fs.rmSync(tempRoot, { recursive: true, force: true }))

  fs.cpSync(path.join(packageRoot, 'bin'), path.join(tempRoot, 'bin'), { recursive: true })

  const [candidate] = getBinCandidates()
  const [scope, name, ...binarySegments] = candidate.split('/')
  const packageDir = path.join(tempRoot, 'node_modules', scope, name)
  const binaryPath = path.join(packageDir, ...binarySegments)
  fs.mkdirSync(packageDir, { recursive: true })
  fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({ name: `${scope}/${name}`, version: '0.0.0' }))
  fs.writeFileSync(
    binaryPath,
    '#!/usr/bin/env node\n' +
    'const fs = require("node:fs");\n' +
    'fs.writeFileSync(process.env.PNPM_SHIM_CAPTURE, JSON.stringify(process.argv.slice(2)));\n'
  )
  fs.chmodSync(binaryPath, 0o755)

  const capturePath = path.join(tempRoot, 'argv.json')
  const { spawnSync } = await import('node:child_process')
  const pnpmResult = spawnSync(process.execPath, [path.join(tempRoot, 'bin/pnpm.mjs'), '--version'], {
    env: { ...process.env, PNPM_SHIM_CAPTURE: capturePath },
    stdio: 'pipe',
  })
  assert.equal(pnpmResult.status, 0, pnpmResult.stderr.toString())
  assert.deepEqual(JSON.parse(fs.readFileSync(capturePath, 'utf8')), ['--version'])

  const pnpxResult = spawnSync(process.execPath, [path.join(tempRoot, 'bin/pnpx.mjs'), 'is-sorted'], {
    env: { ...process.env, PNPM_SHIM_CAPTURE: capturePath },
    stdio: 'pipe',
  })
  assert.equal(pnpxResult.status, 0, pnpxResult.stderr.toString())
  assert.deepEqual(JSON.parse(fs.readFileSync(capturePath, 'utf8')), ['dlx', 'is-sorted'])
})
