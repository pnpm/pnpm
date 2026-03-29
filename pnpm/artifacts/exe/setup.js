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

if (platform === 'win') {
  // On Windows, also hardlink the binary as 'pnpm' and 'pn' (no .exe
  // extension). npm's bin shims point to the name from publishConfig.bin,
  // and npm does NOT re-read package.json after preinstall, so rewriting
  // the bin entry has no effect on the shims. The file at the original
  // name must be the real binary so the shim can execute it.
  linkSync(bin, path.resolve(ownDir, 'pnpm'))
  linkSync(bin, path.resolve(ownDir, 'pn'))

  const pkgJsonPath = path.resolve(ownDir, 'package.json')
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  pkg.bin.pnpm = 'pnpm.exe'
  pkg.bin.pn = 'pn.exe'
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
