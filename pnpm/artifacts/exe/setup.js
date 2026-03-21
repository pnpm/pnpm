import { fileURLToPath } from 'url'
import path from 'path'
import fs from 'fs'

const platform = process.platform === 'win32'
  ? 'win'
  : process.platform === 'darwin'
  ? 'macos'
  : process.platform
const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch

const pkgName = `@pnpm/${platform}-${arch}`
const pkgJson = fileURLToPath(import.meta.resolve(`${pkgName}/package.json`))
const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
const platformDir = path.dirname(pkgJson)
const bin = path.resolve(platformDir, executable)

const ownDir = import.meta.dirname

if (!fs.existsSync(bin)) process.exit(0)

linkSync(bin, path.resolve(ownDir, executable))

// Create pn alias (hardlink to the same binary)
const pnExecutable = platform === 'win' ? 'pn.exe' : 'pn'
linkSync(bin, path.resolve(ownDir, pnExecutable))

// Create pnpx and pnx scripts
createShellScript(ownDir, 'pnpx', 'pnpm dlx')
createShellScript(ownDir, 'pnx', 'pnpm dlx')

if (platform === 'win') {
  const pkgJsonPath = path.resolve(ownDir, 'package.json')
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  fs.writeFileSync(path.resolve(ownDir, 'pnpm'), 'This file intentionally left blank')
  fs.writeFileSync(path.resolve(ownDir, 'pn'), 'This file intentionally left blank')
  pkg.bin.pnpm = 'pnpm.exe'
  pkg.bin.pn = 'pn.exe'
  fs.writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2))
}

function linkSync(src, dest) {
  try {
    fs.unlinkSync(dest)
  } catch (e) {
    if (e.code !== 'ENOENT') {
      throw e
    }
  }
  return fs.linkSync(src, dest)
}

function createShellScript(dir, name, command) {
  fs.writeFileSync(path.resolve(dir, name), `#!/bin/sh\nexec ${command} "$@"\n`, { mode: 0o755 })

  if (platform === 'win') {
    fs.writeFileSync(path.resolve(dir, name + '.cmd'), `@echo off\n${command} %*\n`)
    fs.writeFileSync(path.resolve(dir, name + '.ps1'), `${command} @args\n`)
  }
}
