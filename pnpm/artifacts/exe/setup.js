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

// Create pn, pnpx, and pnx as shell script aliases
createShellScript(ownDir, 'pn', 'pnpm')
createShellScript(ownDir, 'pnpx', 'pnpm dlx')
createShellScript(ownDir, 'pnx', 'pnpm dlx')

if (platform === 'win') {
  // On Windows, also hardlink the binary as 'pnpm' (no .exe extension).
  // npm's bin shims point to the name from publishConfig.bin, and npm
  // does NOT re-read package.json after preinstall, so rewriting the bin
  // entry has no effect on the shims. The file at the original name must
  // be the real binary so the shim can execute it.
  linkSync(bin, path.resolve(ownDir, 'pnpm'))

  const pkgJsonPath = path.resolve(ownDir, 'package.json')
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  pkg.bin.pnpm = 'pnpm.exe'
  pkg.bin.pn = 'pn.cmd'
  pkg.bin.pnpx = 'pnpx.cmd'
  pkg.bin.pnx = 'pnx.cmd'
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
  const file = path.resolve(dir, name)
  try { fs.unlinkSync(file) } catch {}
  fs.writeFileSync(file, `#!/bin/sh\nexec ${command} "$@"\n`, { mode: 0o755 })

  if (platform === 'win') {
    fs.writeFileSync(path.resolve(dir, name + '.cmd'), `@echo off\n${command} %*\n`)
    fs.writeFileSync(path.resolve(dir, name + '.ps1'), `${command} @args\n`)
  }
}
