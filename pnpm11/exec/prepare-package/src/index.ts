import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { AllowBuild, DepPath, PackageManifest } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { preferredPM } from 'preferred-pm'

// We don't run prepublishOnly to prepare the dependency.
// This might be counterintuitive as prepublishOnly is where a lot of packages put their build scripts.
// However, neither npm nor Yarn run prepublishOnly of git-hosted dependencies (checked on npm v10 and Yarn v3).
const PREPUBLISH_SCRIPTS = [
  'prepublish',
  'prepack',
  'publish',
]

// The install that prepares a git-hosted dependency runs in a temporary
// checkout, where nobody can approve the build scripts of that dependency's
// own dependencies. Unapproved builds are skipped there, as they are without
// strictDepBuilds, rather than failing the outer install. Both spellings are
// set because pnpm reads either, and a user's own variable in the other one
// must not win.
const PREPARE_ENV = {
  pnpm_config_strict_dep_builds: 'false',
  PNPM_CONFIG_STRICT_DEP_BUILDS: 'false',
}

export interface PreparePackageOptions {
  allowBuild?: AllowBuild
  ignoreScripts?: boolean
  pkgResolutionId: string
  unsafePerm?: boolean
  userAgent?: string
}

export async function preparePackage (opts: PreparePackageOptions, gitRootDir: string, subDir: string): Promise<{ shouldBeBuilt: boolean, pkgDir: string, ignoredBuild?: boolean }> {
  const pkgDir = safeJoinPath(gitRootDir, subDir)
  const manifest = await safeReadPackageJsonFromDir(pkgDir)
  if (manifest?.scripts == null || !packageShouldBeBuilt(manifest, pkgDir)) return { shouldBeBuilt: false, pkgDir }
  if (opts.ignoreScripts || !resolvePackageBuildPermission(opts, manifest)) {
    return { shouldBeBuilt: true, pkgDir, ignoredBuild: true }
  }
  const pm = (await preferredPM(gitRootDir))?.name ?? 'npm'
  const execOpts: RunLifecycleHookOptions = {
    depPath: `${manifest.name}@${manifest.version}`,
    extraEnv: PREPARE_ENV,
    pkgRoot: pkgDir,
    rootModulesDir: pkgDir, // We don't need this property but there is currently no way to not set it.
    unsafePerm: Boolean(opts.unsafePerm),
    userAgent: opts.userAgent,
  }
  try {
    await runPrepareScripts(pm, manifest, execOpts)
  } catch (err: unknown) {
    assert(isError(err))
    Object.assign(err, {
      code: 'ERR_PNPM_PREPARE_PACKAGE',
    })
    throw err
  }
  await rimraf(path.join(pkgDir, 'node_modules'))
  return { shouldBeBuilt: true, pkgDir }
}

async function runPrepareScripts (
  pm: string,
  manifest: PackageManifest,
  execOpts: RunLifecycleHookOptions
): Promise<void> {
  const installScriptName = `${pm}-install`
  manifest.scripts![installScriptName] = `${pm} install`
  await runLifecycleHook(installScriptName, manifest, execOpts)
  for (const scriptName of PREPUBLISH_SCRIPTS) {
    if (manifest.scripts![scriptName] == null || manifest.scripts![scriptName] === '') continue
    const newScriptName = pm !== 'pnpm' ? `${pm}-run-${scriptName}` : scriptName
    if (pm !== 'pnpm') {
      manifest.scripts![newScriptName] = `${pm} run ${scriptName}`
    }
    // eslint-disable-next-line no-await-in-loop -- prepublish scripts run in the order npm defines
    await runLifecycleHook(newScriptName, manifest, execOpts)
  }
}

export function resolvePackageBuildPermission (
  opts: Pick<PreparePackageOptions, 'allowBuild' | 'pkgResolutionId'>,
  manifest: Partial<PackageManifest>
): boolean {
  const depPath = `${manifest.name}@${opts.pkgResolutionId}` as DepPath
  const allowed = opts.allowBuild?.(depPath)
  if (allowed == null) {
    throw new PnpmError(
      'GIT_DEP_PREPARE_NOT_ALLOWED',
      `The git-hosted package "${manifest.name}@${manifest.version}" needs to execute build scripts but is not in the "allowBuilds" allowlist.`,
      {
        hint: `Add the package to "allowBuilds" in your project's pnpm-workspace.yaml to allow it to run scripts. For example:
allowBuilds:
  ${depPath}: true`,
      }
    )
  }
  return allowed
}

function packageShouldBeBuilt (manifest: PackageManifest, pkgDir: string): boolean {
  if (manifest.scripts == null) return false
  const scripts = manifest.scripts
  if (scripts.prepare != null && scripts.prepare !== '') return true
  const hasPrepublishScript = PREPUBLISH_SCRIPTS.some((scriptName) => scripts[scriptName] != null && scripts[scriptName] !== '')
  if (!hasPrepublishScript) return false
  const mainFile = manifest.main ?? 'index.js'
  return !fs.existsSync(path.join(pkgDir, mainFile))
}

function safeJoinPath (root: string, sub: string): string {
  const normalizedSub = sub.replace(/^[/\\]+/, '')
  const joined = normalizedSub === '' ? root : path.join(root, normalizedSub)
  // Prevent directory traversal attacks lexically first
  const lexicalRelative = path.relative(root, joined)
  if (lexicalRelative === '..' || lexicalRelative.startsWith(`..${path.sep}`) || path.isAbsolute(lexicalRelative)) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" should be a sub directory`)
  }
  let realRoot: string
  let realJoined: string
  try {
    realRoot = fs.realpathSync(root)
    realJoined = fs.realpathSync(joined)
  } catch (err: unknown) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" is not a directory`, { cause: err })
  }
  // Verify that any intermediate symlinks or resolved target remain inside the repository root
  const relative = path.relative(realRoot, realJoined)
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" should be a sub directory`)
  }
  if (!fs.statSync(realJoined).isDirectory()) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" is not a directory`)
  }
  return joined
}
