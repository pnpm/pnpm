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
  // pnpm v11 dropped the darwin-x64 artifact from @pnpm/exe because Node.js
  // SEA injection produces a binary that segfaults at startup on Intel Macs
  // (pnpm/pnpm#11423, upstream nodejs/node#62893). Self-updating a v10
  // @pnpm/exe install to v11+ on Intel Mac would leave the user with no
  // working binary. Transparently swap to the JS-only `pnpm` package, which
  // runs against the user's system Node.js — typically already present
  // because v10 @pnpm/exe users tend to manage Node via `pnpm env use`.
  const targetPkgName = (
    currentPkgName === '@pnpm/exe' &&
    process.platform === 'darwin' &&
    process.arch === 'x64' &&
    semver.major(pnpmVersion) >= 11
  )
    ? 'pnpm'
    : currentPkgName
  if (targetPkgName !== currentPkgName) {
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
    if (targetPkgName === '@pnpm/exe') {
      linkExePlatformBinary(stage)
    }
    // Reached only when the wanted version is not yet in the tools directory
    // (an actual download), so the signature check does not run on every
    // invocation. Verify before the staged install is linked into place and
    // spawned — on failure the stage is removed by the catch below.
    await verifyPnpmEngineIdentity(stage, targetPkgName, pnpmVersion, opts)
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

// This replicates the logic from @pnpm/exe's setup.js (pnpm/artifacts/exe/setup.js).
// We can't run setup.js via require() or import() because:
// - require() fails when setup.js is ESM (pnpm v11+)
// - import() is intercepted by pkg's virtual filesystem in standalone executables
// So we inline the logic: find the platform-specific binary and hard-link it
// into the @pnpm/exe package directory.
function linkExePlatformBinary (stageDir: string): void {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
  const platformPkgDir = path.join(stageDir, 'node_modules', '@pnpm', `${platform}-${arch}`)
  const src = path.join(platformPkgDir, executable)
  if (!fs.existsSync(src)) return
  const exePkgDir = path.join(stageDir, 'node_modules', '@pnpm', 'exe')
  const dest = path.join(exePkgDir, executable)
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)
  if (platform === 'win') {
    const exePkgJsonPath = path.join(exePkgDir, 'package.json')
    const exePkg = JSON.parse(fs.readFileSync(exePkgJsonPath, 'utf8'))
    fs.writeFileSync(path.join(exePkgDir, 'pnpm'), 'This file intentionally left blank')
    exePkg.bin.pnpm = 'pnpm.exe'
    fs.writeFileSync(exePkgJsonPath, JSON.stringify(exePkg, null, 2))
  }
}
