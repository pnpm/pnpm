import { fileURLToPath } from 'url'
import path from 'path'
import fs from 'fs'
import { spawnSync } from 'child_process'
import { familySync } from 'detect-libc'
import { exePlatformPkgName, missingPlatformPkgMessage } from './platform-pkg-name.js'

if (process.env.npm_lifecycle_event === 'postinstall') {
  relinkNpmWindowsShims()
  process.exit(0)
}

// Platform package names use the legacy scheme: `@pnpm/macos-<arch>` (darwin),
// `@pnpm/win-<arch>` (win32), `@pnpm/linux-<arch>` (glibc), and
// `@pnpm/linuxstatic-<arch>` (musl Linux, detected via detect-libc). This is
// the naming published on npm, even though the workspace directories use the
// newer `<os>-<arch>[-musl]` scheme. Keeping these names lets `pnpm
// self-update` from older majors continue to resolve the right platform child.
// The name computation lives in platform-pkg-name.js so it can be unit-tested
// without triggering the side effects of this preinstall script.
const platform = process.platform
const libcFamily = familySync()
const pkgName = exePlatformPkgName(platform, process.arch, libcFamily)
let pkgJson
try {
  pkgJson = fileURLToPath(import.meta.resolve(`${pkgName}/package.json`))
} catch (err) {
  // Only treat ERR_MODULE_NOT_FOUND as "platform package not installed".
  // Anything else (resolver bug, broken Node, etc.) should surface as-is.
  if (err?.code !== 'ERR_MODULE_NOT_FOUND') throw err

  // The platform package isn't on disk: @pnpm/exe deliberately ships no
  // binary for some hosts (see missingPlatformPkgMessage).
  //
  // Inside the pnpm workspace itself there's no platform package linked
  // either, since the workspace packages for those hosts were removed. We
  // don't want a contributor on such a host blocked from `pnpm install`-ing
  // the repo to work on unrelated parts of pnpm, so skip silently when this
  // script runs as the workspace's own @pnpm/exe (whose path always ends in
  // pnpm/artifacts/exe). A path-suffix check is more precise than walking
  // up for `pnpm-workspace.yaml` — that walk can false-positive if the
  // user's globally-installed @pnpm/exe happens to live anywhere under
  // an unrelated pnpm workspace tree.
  if (import.meta.dirname.endsWith(path.join('pnpm', 'artifacts', 'exe'))) {
    process.exit(0)
  }

  console.error(missingPlatformPkgMessage(platform, process.arch, libcFamily))
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
  // does NOT re-read package.json after preinstall. This original target
  // remains executable until postinstall regenerates npm's shims.
  linkSync(bin, path.resolve(ownDir, 'pnpm'))

  // Aliases (pn / pnpx / pnx) need to be .exe hardlinks of the SEA binary,
  // not the .cmd / .ps1 wrappers we ship in the tarball. Reason: in MSYS2 /
  // Git Bash, cmd-shim's Bash shim for a .cmd target does
  //   exec cmd /C "...target.cmd" "$@"
  // and MSYS2 mangles the lone `/C` switch into a Windows path before
  // cmd.exe sees it — cmd.exe then finds no /C or /K and falls into
  // interactive mode, printing its banner instead of the alias. .exe
  // sources sidestep cmd-shim's wrapper. The SEA binary detects which name
  // it was launched as via process.execPath and prepends `dlx` for
  // pnpx / pnx. See https://github.com/pnpm/pnpm/issues/11486.
  for (const alias of ['pn', 'pnpx', 'pnx']) {
    linkSync(bin, path.resolve(ownDir, `${alias}.exe`))
  }

  const pkgJsonPath = path.resolve(ownDir, 'package.json')
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  pkg.bin.pnpm = 'pnpm.exe'
  pkg.bin.pn = 'pn.exe'
  pkg.bin.pnpx = 'pnpx.exe'
  pkg.bin.pnx = 'pnx.exe'
  fs.writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2))
}

function relinkNpmWindowsShims() {
  const npmExecPath = process.env.npm_execpath
  if (
    process.platform !== 'win32' ||
    npmExecPath == null ||
    path.basename(npmExecPath).toLowerCase() !== 'npm-cli.js'
  ) return

  const args = [npmExecPath, 'rebuild', '--ignore-scripts']
  if (process.env.npm_config_global === 'true' || process.env.npm_config_location === 'global') {
    args.push('--global')
    if (process.env.npm_config_prefix) {
      args.push('--prefix', process.env.npm_config_prefix)
    }
  } else {
    // The script runs inside the installed package, so a project install
    // names its project explicitly.
    const projectPrefix = findNpmProjectPrefix()
    if (projectPrefix == null) return
    args.push('--prefix', projectPrefix)
  }
  args.push('@pnpm/exe')
  const result = spawnSync(process.execPath, args, { stdio: 'inherit' })
  if (result.error != null) {
    console.error(`Could not regenerate the npm shims for @pnpm/exe: ${result.error.message}`)
    process.exit(1)
  }
  if (result.status !== 0) {
    console.error(`npm could not regenerate the shims for @pnpm/exe (exit code ${result.status}).`)
    process.exit(1)
  }
}

/**
 * The resolved path of the npm project whose `node_modules` holds this
 * package. Returns `null` when npm names no project, when its `node_modules`
 * cannot be resolved, or when it does not contain the package. `npm exec`
 * installs into its own cache while the prefix still names the caller's
 * project, and rebuilding there would touch an unrelated project.
 */
function findNpmProjectPrefix() {
  const prefix = process.env.npm_config_local_prefix
  if (!prefix) return null
  let realPrefix
  let realModulesDir
  try {
    realPrefix = fs.realpathSync(prefix)
    realModulesDir = fs.realpathSync(path.join(realPrefix, 'node_modules'))
  } catch {
    return null
  }
  // import.meta.dirname is resolved through symlinks.
  const relative = path.relative(realModulesDir, import.meta.dirname)
  if (relative === '' || relative.split(path.sep)[0] === '..' || path.isAbsolute(relative)) return null
  return realPrefix
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
