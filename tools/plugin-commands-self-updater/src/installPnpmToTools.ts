import fs from 'fs'
import path from 'path'
import { getCurrentPackageName } from '@pnpm/cli-meta'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { globalWarn } from '@pnpm/logger'
import { getToolDirPath } from '@pnpm/tools.path'
import { sync as rimraf } from '@zkochan/rimraf'
import { fastPathTemp as pathTemp } from 'path-temp'
import semver from 'semver'
import symlinkDir from 'symlink-dir'
import { type SelfUpdateCommandOptions } from './selfUpdate.js'
import { verifyPnpmEngineIdentity } from './verifyPnpmEngineIdentity.js'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export async function installPnpmToTools (pnpmVersion: string, opts: SelfUpdateCommandOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()
  const targetPkgName = pnpmPackageNameToInstall(pnpmVersion, currentPkgName)
  // The v11-only darwin-x64 fallback (see pnpmPackageNameToInstall) swaps the
  // native @pnpm/exe for the JS `pnpm` package, which needs Node.js on PATH —
  // warn about that. From v12 the `pnpm` package is itself native, so the
  // convergence to `pnpm` is transparent and needs no warning.
  if (targetPkgName !== currentPkgName && semver.major(pnpmVersion) < 12) {
    globalWarn(
      `Switching from @pnpm/exe to the "pnpm" npm package because @pnpm/exe v${pnpmVersion} no longer ships a binary for Intel macOS (darwin-x64). The new "pnpm" install requires Node.js to be available on PATH. See https://github.com/pnpm/pnpm/issues/11423.`
    )
  }
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: targetPkgName,
      version: pnpmVersion,
    },
  })

  const binDir = path.join(dir, 'bin')
  const alreadyExisted = fs.existsSync(binDir)
  if (alreadyExisted) {
    return {
      alreadyExisted,
      baseDir: dir,
      binDir,
    }
  }
  const stage = pathTemp(dir)
  fs.mkdirSync(stage, { recursive: true })
  fs.writeFileSync(path.join(stage, 'package.json'), '{}')
  try {
    // The reason we don't just run add.handler is that at this point we might have settings from local config files
    // that we don't want to use while installing the pnpm CLI.
    // We use --ignore-scripts because `@pnpm/exe` has a `preinstall` script that runs `node setup.js`,
    // which fails in environments without a system Node.js (e.g. when pnpm is installed as a standalone executable).
    // Instead, we link the platform-specific binary in-process after install.
    runPnpmCli([
      'add',
      `${targetPkgName}@${pnpmVersion}`,
      '--loglevel=error',
      '--ignore-scripts',
      '--config.strict-dep-builds=false',
      // We want to avoid symlinks because of the rename step,
      // which breaks the junctions on Windows.
      '--config.node-linker=hoisted',
      '--config.bin=bin',
      // This is an isolated install into `stage` and must not inherit the
      // caller's workspace context. Without this, the child's workspace
      // walk-up from `stage` can discover an ancestor pnpm-workspace.yaml
      // and treat the caller's project as the workspace root — breaking the
      // add (it's outside the workspace's packages list) and, before the env
      // guards below existed, picking up the caller's packageManager field
      // and re-entering switchCliVersion for a fork bomb. See pnpm/pnpm#11337.
      '--ignore-workspace',
    ], {
      cwd: stage,
      // Defense in depth against re-entering switchCliVersion in the child,
      // in case any future code path surfaces a wantedPackageManager without
      // going through workspace discovery. Both env-var names are set so the
      // guard works regardless of whether the child reads pnpm's v10
      // (npm_config_*) or v11+ (pnpm_config_*) convention.
      env: {
        ...process.env,
        npm_config_manage_package_manager_versions: 'false',
        pnpm_config_pm_on_fail: 'ignore',
      },
    })
    // Reached only when the wanted version is not yet in the tools directory
    // (an actual download), so the signature check does not run on every
    // invocation. Verify BEFORE relinking (below) and before the staged install
    // is linked into place and spawned: relinking mutates files whose names come
    // from the wrapper's own (still untrusted) package.json, so it must not run
    // until the wrapper is proven to be a genuine, signed pnpm release. On
    // failure the stage is removed by the catch below.
    await verifyPnpmEngineIdentity(stage, targetPkgName, pnpmVersion, opts)
    // pnpm's own installs run with --ignore-scripts, so the wrapper's
    // preinstall (which links the native binary over the placeholder bin) never
    // runs — replicate it here. Needed for any wrapper that ships a native
    // binary: `@pnpm/exe` (all majors) and, from v12, the `pnpm` package too.
    if (targetPkgName === '@pnpm/exe' || semver.major(pnpmVersion) >= 12) {
      linkExePlatformBinary(stage, targetPkgName)
    }
    // We need the operation of installing pnpm to be atomic.
    // However, we cannot use a rename as that breaks the command shim created for pnpm.
    // Hence, we use a symlink.
    // In future we may switch back to rename if we will move Node.js out of the pnpm subdirectory.
    symlinkDir.sync(stage, dir)
  } catch (err: unknown) {
    try {
      rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
  return {
    alreadyExisted,
    baseDir: dir,
    binDir,
  }
}

/**
 * The package to install for a switch to `pnpmVersion`.
 *
 * From v12 the unscoped `pnpm` package is itself the native executable (equal
 * content to `@pnpm/exe`) and ships a binary for every target — including
 * darwin-x64 — so v12+ always converges on `pnpm`, even from a standalone
 * `@pnpm/exe` build.
 *
 * For earlier majors the running package name is kept, except that a v11+
 * update of a darwin-x64 `@pnpm/exe` install falls back to the JS `pnpm`
 * package: pnpm v11+ ships no darwin-x64 artifact for `@pnpm/exe` because
 * Node.js SEA injection produces a binary that segfaults at startup on Intel
 * Macs (pnpm/pnpm#11423, upstream nodejs/node#62893), which would leave the
 * user with no working binary. The JS `pnpm` package runs against the user's
 * system Node.js — typically already present because v10 `@pnpm/exe` users
 * tend to manage Node via `pnpm env use`.
 */
export function pnpmPackageNameToInstall (pnpmVersion: string, currentPkgName: string): string {
  const major = semver.major(pnpmVersion)
  if (major >= 12) return 'pnpm'
  if (
    currentPkgName === '@pnpm/exe' &&
    process.platform === 'darwin' &&
    process.arch === 'x64' &&
    major >= 11
  ) {
    return 'pnpm'
  }
  return currentPkgName
}

// This replicates the logic from the wrapper's preinstall script (the
// TypeScript @pnpm/exe's setup.js and the Rust pnpm v12's install.js), which
// we skip via --ignore-scripts. We can't just run the script because:
// - require() fails when it is ESM (pnpm v11+)
// - import() is intercepted by pkg's virtual filesystem in standalone executables
// So we inline it: find the host's native binary and hard-link it over the
// wrapper's placeholder bin. Handles both the legacy `@pnpm/<os>-<arch>`
// platform packages (v10/v11 @pnpm/exe) and the v12 `@pnpm/exe.<platform>-<arch>[-musl]`
// scheme (shared by both the `pnpm` and `@pnpm/exe` wrappers).
export function linkExePlatformBinary (stageDir: string, wrapperPkgName: string): void {
  const wrapperDir = path.join(stageDir, 'node_modules', ...wrapperPkgName.split('/'))
  if (!fs.existsSync(wrapperDir)) return
  const scopeDir = path.join(stageDir, 'node_modules', '@pnpm')
  const isWin = process.platform === 'win32'
  const executable = isWin ? 'pnpm.exe' : 'pnpm'
  // Only the platform package matching the host's os/cpu/libc is installed, so
  // link whichever candidate is actually present on disk.
  let src: string | undefined
  for (const dirName of exePlatformPkgDirNames(process.platform, process.arch)) {
    const candidate = path.join(scopeDir, dirName, executable)
    if (fs.existsSync(candidate)) {
      src = candidate
      break
    }
  }
  if (src == null) return

  if (!isWin) {
    // On Unix `pn`/`pnpx`/`pnx` are committed `#!/bin/sh` scripts that call
    // `pnpm` on PATH, so only the `pnpm` placeholder needs the native binary.
    forceLink(src, path.join(wrapperDir, 'pnpm'))
    return
  }

  const wrapperPkgJsonPath = path.join(wrapperDir, 'package.json')
  const wrapperPkg = JSON.parse(fs.readFileSync(wrapperPkgJsonPath, 'utf8'))
  const bin: Record<string, string> = (typeof wrapperPkg.bin === 'object' && wrapperPkg.bin != null)
    ? wrapperPkg.bin
    : { pnpm: 'pnpm' }
  for (const name of safeWrapperBinNames(wrapperDir, bin)) {
    // Link the native binary onto both `<name>.exe` (cmd.exe resolves the
    // extension-less shim target through PATHEXT) and the extension-less file
    // (Git Bash runs it directly), matching the wrapper's own install.js. The
    // native binary self-detects its launch name to inject `dlx` for pnpx/pnx.
    forceLink(src, path.join(wrapperDir, `${name}.exe`))
    forceLink(src, path.join(wrapperDir, name))
    bin[name] = `${name}.exe`
  }
  wrapperPkg.bin = bin
  // Temp file + rename, not in-place: package.json is hard-linked from the
  // content-addressable store, so writing in place would mutate the shared blob.
  const tempPkgJsonPath = `${wrapperPkgJsonPath}.pnpm-tmp`
  try {
    fs.writeFileSync(tempPkgJsonPath, JSON.stringify(wrapperPkg, null, 2))
    fs.renameSync(tempPkgJsonPath, wrapperPkgJsonPath)
  } catch (err: unknown) {
    try {
      fs.rmSync(tempPkgJsonPath, { force: true })
    } catch {}
    throw err
  }
}

/**
 * Directory names under `node_modules/@pnpm/` of every platform package that
 * could carry the native pnpm binary for this host, most-preferred first —
 * both the legacy `@pnpm/<os>-<arch>` names (v10/v11 @pnpm/exe) and the v12
 * `@pnpm/exe.<platform>-<arch>[-musl]` names. Only the package matching the
 * host's os/cpu/libc is ever installed, so the caller links the first that
 * exists on disk; enumerating both libc variants avoids detecting libc here.
 */
export function exePlatformPkgDirNames (platform: NodeJS.Platform, arch: string): string[] {
  switch (platform) {
  case 'win32': {
    const legacyArch = arch === 'ia32' ? 'x86' : arch
    return [`win-${legacyArch}`, `exe.win32-${arch}`]
  }
  case 'darwin':
    return [`macos-${arch}`, `exe.darwin-${arch}`]
  case 'linux':
    return [
      `linux-${arch}`, // legacy glibc
      `linuxstatic-${arch}`, // legacy musl
      `exe.linux-${arch}`, // v12 glibc
      `exe.linux-${arch}-musl`, // v12 musl
    ]
  default:
    return [`${platform}-${arch}`, `exe.${platform}-${arch}`]
  }
}

/**
 * The bin names from a wrapper's `package.json` that are safe to relink. The
 * manifest is not signature-verified at the point of relinking (defense in
 * depth behind the verify-before-relink ordering in installPnpmToTools), so a
 * crafted `bin` map must not be able to traverse out of the wrapper directory
 * via `..`, path separators, or absolute paths: only names that resolve to a
 * direct child of `wrapperDir` are kept.
 */
export function safeWrapperBinNames (wrapperDir: string, bin: Record<string, string>): string[] {
  const resolvedWrapperDir = path.resolve(wrapperDir)
  return Object.keys(bin).filter((name) => path.dirname(path.resolve(wrapperDir, name)) === resolvedWrapperDir)
}

function forceLink (src: string, dest: string): void {
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)
}
