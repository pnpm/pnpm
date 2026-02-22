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
const subpkg = JSON.parse(fs.readFileSync(pkgJson, 'utf8'))

if (subpkg.bin != null) {
  const executable = subpkg.bin.pnpm
  const platformDir = path.dirname(pkgJson)
  const bin = path.resolve(platformDir, executable)

  const ownDir = import.meta.dirname

  linkSync(bin, path.resolve(ownDir, executable))

  if (platform === 'win') {
    const pkgJsonPath = path.resolve(ownDir, 'package.json')
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
    fs.writeFileSync(path.resolve(ownDir, 'pnpm'), 'This file intentionally left blank')
    pkg.bin.pnpm = 'pnpm.exe'
    fs.writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2))
  }
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
