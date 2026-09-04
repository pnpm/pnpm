import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

const wrapperDir = path.resolve(fileURLToPath(import.meta.url), '../..')
const wrapperManifest = JSON.parse(fs.readFileSync(path.join(wrapperDir, 'package.json'), 'utf8'))

test('npm installs a shim that runs the native pnpm binary', (t) => {
  assert.equal(wrapperManifest.bin.pnpm, 'pnpm')

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm wrapper install-'))
  t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))

  const candidate = getBinCandidates()[0]
  assert.ok(candidate)
  const { packageName, binFile } = splitBinSpecifier(candidate)
  const nativePackageDir = path.join(tempDir, 'native-package')
  fs.mkdirSync(nativePackageDir)
  fs.writeFileSync(path.join(nativePackageDir, 'package.json'), JSON.stringify({
    name: packageName,
    version: '1.0.0',
  }))
  writeNativeFixture(path.join(nativePackageDir, binFile))

  const fixtureDir = path.join(tempDir, 'wrapper')
  fs.mkdirSync(fixtureDir)
  for (const file of ['install.js', 'native-binary.mjs', wrapperManifest.bin.pnpm]) {
    fs.copyFileSync(path.join(wrapperDir, file), path.join(fixtureDir, file))
  }
  fs.writeFileSync(path.join(fixtureDir, 'package.json'), JSON.stringify({
    name: 'pnpm-install-fixture',
    version: '1.0.0',
    type: 'module',
    bin: { pnpm: wrapperManifest.bin.pnpm },
    scripts: {
      preinstall: 'node install.js',
      postinstall: 'node install.js',
    },
    optionalDependencies: { [packageName]: `file:${nativePackageDir}` },
  }))

  const prefix = path.join(tempDir, 'prefix')
  runNpm([
    'install',
    '--global',
    '--install-links=true',
    '--dangerously-allow-all-scripts',
    '--prefix',
    prefix,
    fixtureDir,
  ], tempDir)

  if (process.platform === 'win32') {
    const cmdShim = path.join(prefix, 'pnpm.cmd')
    assert.match(fs.readFileSync(cmdShim, 'utf8'), /pnpm\.exe/)
    assert.match(execFileSync('cmd.exe', ['/d', '/s', '/c', 'call', cmdShim, '--version'], { encoding: 'utf8' }), /^v\d+/)

    const powershellShim = path.join(prefix, 'pnpm.ps1')
    assert.match(fs.readFileSync(powershellShim, 'utf8'), /pnpm\.exe/)
    assert.match(execFileSync('pwsh', ['-NoProfile', '-File', powershellShim, '--version'], { encoding: 'utf8' }), /^v\d+/)
  } else {
    assert.equal(execFileSync(path.join(prefix, 'bin', 'pnpm'), ['works'], { encoding: 'utf8' }), 'fixture:works\n')
  }
})

function runNpm (args, cwd) {
  if (process.platform === 'win32') {
    const npmCli = execFileSync('where.exe', ['npm.cmd'], { encoding: 'utf8' })
      .split(/\r?\n/)
      .filter(Boolean)
      .map(launcher => path.join(path.dirname(launcher), 'node_modules', 'npm', 'bin', 'npm-cli.js'))
      .find(candidate => fs.existsSync(candidate))
    assert.ok(npmCli, 'Unable to find npm-cli.js next to an npm.cmd on PATH')
    execFileSync(process.execPath, [npmCli, ...args], { cwd, stdio: 'pipe' })
  } else {
    execFileSync('npm', args, { cwd, stdio: 'pipe' })
  }
}

test('linux riscv64 resolves the glibc package, and nothing under musl', async (t) => {
  const restore = fakeHost(t, 'riscv64')

  // `native-binary.mjs` reads process.platform and process.arch at module
  // scope, so it has to be imported again once they are faked. The libc is
  // read per call, so one import covers both cases.
  const { getBinCandidates: candidates } = await import('../native-binary.mjs?riscv64')

  restore.setLibc('glibc')
  assert.deepEqual(candidates(), ['@pnpm/exe.linux-riscv64/pnpm'])

  // No musl binary is released for riscv64, and the glibc one cannot run
  // there, so the installer reports an unsupported platform instead.
  restore.setLibc('musl')
  assert.deepEqual(candidates(), [])
})

test('an architecture released for both libcs still offers the other as a fallback', async (t) => {
  const restore = fakeHost(t, 'x64')
  const { getBinCandidates: candidates } = await import('../native-binary.mjs?x64')

  restore.setLibc('glibc')
  assert.deepEqual(candidates(), ['@pnpm/exe.linux-x64/pnpm', '@pnpm/exe.linux-x64-musl/pnpm'])

  restore.setLibc('musl')
  assert.deepEqual(candidates(), ['@pnpm/exe.linux-x64-musl/pnpm', '@pnpm/exe.linux-x64/pnpm'])
})

/**
 * Present the running process as a Linux host of `arch`, restoring the real
 * descriptors when the test ends. `setLibc` drives `detectLinuxLibc`, which
 * reads `process.report`, so the cases do not depend on the host's own libc.
 */
function fakeHost (t, arch) {
  const saved = ['platform', 'arch', 'report']
    .map(key => [key, Object.getOwnPropertyDescriptor(process, key)])
  t.after(() => {
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(process, key, descriptor)
    }
  })
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  Object.defineProperty(process, 'arch', { value: arch, configurable: true })
  return {
    setLibc (libc) {
      const header = libc === 'glibc' ? { glibcVersionRuntime: '2.39' } : {}
      Object.defineProperty(process, 'report', {
        value: { getReport: () => ({ header }) },
        configurable: true,
      })
    },
  }
}

function writeNativeFixture (destPath) {
  if (process.platform === 'win32') {
    try {
      fs.linkSync(process.execPath, destPath)
    } catch (err) {
      if (err.code !== 'EXDEV') throw err
      fs.copyFileSync(process.execPath, destPath)
    }
  } else {
    fs.writeFileSync(destPath, '#!/bin/sh\nprintf \'fixture:%s\\n\' "$1"\n', { mode: 0o755 })
  }
}
