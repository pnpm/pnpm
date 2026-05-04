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
} catch (err) {
  // Only treat ERR_MODULE_NOT_FOUND as "platform package not installed".
  // Anything else (resolver bug, broken Node, etc.) should surface as-is.
  if (err?.code !== 'ERR_MODULE_NOT_FOUND') throw err

  // The platform package isn't on disk. The only currently-published host
  // for which @pnpm/exe deliberately omits a binary is darwin-x64 (Intel
  // Mac): Node.js SEA injection corrupts the binary on x64 Mach-O — see
  // https://github.com/pnpm/pnpm/issues/11423 and upstream
  // https://github.com/nodejs/node/issues/62893.
  //
  // Inside the pnpm workspace itself there's no platform package linked
  // either — it would be `@pnpm/macos-x64` for darwin-x64 and we removed
  // that workspace package entirely. We don't want a contributor on Intel
  // hardware blocked from `pnpm install`-ing the repo to work on
  // unrelated parts of pnpm, so skip silently when this script runs as
  // the workspace's own @pnpm/exe (whose path always ends in
  // pnpm/artifacts/exe). A path-suffix check is more precise than walking
  // up for `pnpm-workspace.yaml` — that walk can false-positive if the
  // user's globally-installed @pnpm/exe happens to live anywhere under
  // an unrelated pnpm workspace tree.
  if (import.meta.dirname.endsWith(path.join('pnpm', 'artifacts', 'exe'))) {
    process.exit(0)
  }

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
