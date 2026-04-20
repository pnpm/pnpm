import { fileURLToPath } from 'url'
import path from 'path'
import fs from 'fs'
import { familySync } from 'detect-libc'
import { exePlatformPkgName } from './platform-pkg-name.js'

// Platform names match process.platform (linux | darwin | win32). On linux,
// add a `-musl` libc suffix when detect-libc reports musl, matching the
// @pnpm/exe.linux-<arch>-musl optional-dep naming. The name computation lives
// in platform-pkg-name.js so it can be unit-tested without triggering the
// side effects of this preinstall script.
const platform = process.platform
const pkgName = exePlatformPkgName(platform, process.arch, familySync())
const pkgJson = fileURLToPath(import.meta.resolve(`${pkgName}/package.json`))
const executable = platform === 'win32' ? 'pnpm.exe' : 'pnpm'
const platformDir = path.dirname(pkgJson)
const bin = path.resolve(platformDir, executable)

const ownDir = import.meta.dirname

if (!fs.existsSync(bin)) process.exit(0)

linkSync(bin, path.resolve(ownDir, executable))

if (platform === 'win32') {
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
