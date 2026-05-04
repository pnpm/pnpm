import { fileURLToPath } from 'url'
import path from 'path'
import fs from 'fs'
import { familySync } from 'detect-libc'
import { exePlatformPkgName } from './platform-pkg-name.js'

// Platform package names use the legacy scheme: `@pnpm/macos-<arch>` (darwin),
// `@pnpm/win-<arch>` (win32), `@pnpm/linux-<arch>` (glibc), and
// `@pnpm/linuxstatic-<arch>` (musl Linux, detected via detect-libc). This is
// the naming published on npm, even though the workspace directories use the
// newer `<os>-<arch>[-musl]` scheme. Keeping these names lets `pnpm
// self-update` from older majors continue to resolve the right platform child.
// The name computation lives in platform-pkg-name.js so it can be unit-tested
// without triggering the side effects of this preinstall script.
const platform = process.platform
const pkgName = exePlatformPkgName(platform, process.arch, familySync())
let pkgJson
try {
  pkgJson = fileURLToPath(import.meta.resolve(`${pkgName}/package.json`))
} catch {
  // No matching platform package was installed. Currently the only host
  // @pnpm/exe deliberately doesn't ship a binary for is darwin-x64 (Intel
  // Mac), where Node.js SEA injection corrupts the binary on x64 Mach-O —
  // see https://github.com/pnpm/pnpm/issues/11423 and upstream
  // https://github.com/nodejs/node/issues/62893. Fail loudly with a clear
  // pointer instead of leaving the user with a placeholder `pnpm` file.
  if (platform === 'darwin' && process.arch === 'x64') {
    console.error(
      '@pnpm/exe does not ship a working binary for Intel macOS (darwin-x64) due to an upstream Node.js SEA bug.\n' +
      'See https://github.com/pnpm/pnpm/issues/11423 and https://github.com/nodejs/node/issues/62893.\n' +
      'Workaround: install pnpm via `npm install -g pnpm` (uses your system Node.js, no SEA), or use pnpm 10.x.'
    )
  } else {
    console.error(`Could not find platform package "${pkgName}" — @pnpm/exe does not ship a binary for ${platform}-${process.arch}.`)
  }
  process.exit(1)
}
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
