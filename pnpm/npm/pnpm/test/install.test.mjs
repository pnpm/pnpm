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
    const shim = path.join(prefix, 'pnpm.ps1')
    assert.match(fs.readFileSync(shim, 'utf8'), /pnpm\.exe/)
    assert.match(execFileSync('pwsh', ['-NoProfile', '-File', shim, '--version'], { encoding: 'utf8' }), /^v\d+/)
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
