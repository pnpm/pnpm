import { existsSync } from 'node:fs'
import path from 'node:path'

import { lifecycleLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { appendScriptArgs, lifecycle, type LifecyclePackage, scriptSearchPath, showScriptWithArgs } from '@pnpm/exec.npm-lifecycle'
import { globalWarn } from '@pnpm/logger'
import type { DependencyManifest, PackageScripts, ProjectManifest } from '@pnpm/types'
import chalk from 'chalk'
import isWindows from 'is-windows'

import { trackChildProcess } from './trackChildProcess.js'

function noop () {}

export interface RunLifecycleHookOptions {
  args?: string[]
  depPath: string
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  initCwd?: string
  optional?: boolean
  pkgRoot: string
  raiseOnInterrupt?: boolean
  rootModulesDir: string
  /**
   * The `.bin` holding `pkgRoot`'s own executables, when `modulesDir` puts
   * them somewhere other than `<pkgRoot>/node_modules/.bin`.
   */
  wdBinDir?: string
  scriptShell?: string
  silent?: boolean
  scriptsPrependNodePath?: boolean | 'warn-only'
  shellEmulator?: boolean
  stdio?: 'inherit' | 'pipe'
  unsafePerm: boolean
  userAgent?: string
}

export async function runLifecycleHook (
  stage: string,
  manifest: ProjectManifest | DependencyManifest,
  opts: RunLifecycleHookOptions
): Promise<boolean> {
  const optional = opts.optional === true
  checkScriptShell(opts.scriptShell)

  const hookManifest = prepareHookManifest(manifest, stage, opts.pkgRoot)
  const scriptShell = typeof opts.scriptShell === 'string' && opts.scriptShell !== '' ? opts.scriptShell : undefined
  const shownScript = resolveShownScript(hookManifest, stage, opts, scriptShell)

  // This script is used to prevent the usage of npm or Yarn.
  // It does nothing, when pnpm is used, so we may skip its execution.
  if (hookManifest.scripts[stage] === 'npx only-allow pnpm' || !hookManifest.scripts[stage]) return false
  logLifecycleStart({ depPath: opts.depPath, optional, pkgRoot: opts.pkgRoot, script: shownScript, silent: opts.silent, stage, stdio: opts.stdio })

  await executeLifecycle({ hookManifest, opts, optional, scriptShell, shownScript, stage })
  return true
}

function checkScriptShell (scriptShell?: string): void {
  if (scriptShell != null && typeof scriptShell === 'string' && isWindowsBatchFile(scriptShell)) {
    throw new PnpmError('ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS', 'Cannot spawn .bat or .cmd as a script shell.', {
      hint: `\
The pnpm-workspace.yaml scriptShell option was configured to a .bat or .cmd file. These cannot be used as a script shell reliably.

Please unset the scriptShell option, or configure it to a .exe instead.
`,
    })
  }
}

type HookManifest = LifecyclePackage & { scripts: PackageScripts }

function prepareHookManifest (
  manifest: ProjectManifest | DependencyManifest,
  stage: string,
  pkgRoot: string
): HookManifest {
  const scripts: PackageScripts = { ...manifest.scripts }
  const hookManifest: HookManifest = { ...manifest, _id: getId(manifest), scripts }

  switch (stage) {
    case 'start':
      if (!hookManifest.scripts.start) {
        if (!existsSync('server.js')) {
          throw new PnpmError('NO_SCRIPT_OR_SERVER', 'Missing script start or file server.js')
        }
        hookManifest.scripts.start = 'node server.js'
      }
      break
    case 'install':
      if (!hookManifest.scripts.install && !hookManifest.scripts.preinstall && hookManifest.gypfile !== false) {
        checkBindingGyp(pkgRoot, hookManifest.scripts)
      }
      break
  }
  return hookManifest
}

function resolveShownScript (
  hookManifest: HookManifest,
  stage: string,
  opts: RunLifecycleHookOptions,
  scriptShell?: string
): string {
  let shownScript = hookManifest.scripts[stage]
  if (opts.args?.length && hookManifest.scripts?.[stage]) {
    shownScript = showScriptWithArgs(hookManifest.scripts[stage], opts.args)
    hookManifest.scripts[stage] = appendScriptArgs(hookManifest.scripts[stage], opts.args, {
      platform: isWindows() ? 'win32' : 'linux',
      scriptShell,
      shellEmulator: opts.shellEmulator,
      wd: opts.pkgRoot,
      searchPath: () => scriptSearchPath(opts.pkgRoot, opts),
    })
  }
  return shownScript
}

interface LogStartParams {
  depPath: string
  optional: boolean
  pkgRoot: string
  script: string
  silent?: boolean
  stage: string
  stdio?: 'inherit' | 'pipe'
}

function logLifecycleStart (params: LogStartParams): void {
  if (params.stdio !== 'inherit') {
    lifecycleLogger.debug({
      depPath: params.depPath,
      optional: params.optional,
      script: params.script,
      stage: params.stage,
      wd: params.pkgRoot,
    })
  } else if (!params.silent) {
    process.stderr.write(chalk.dim(`$ ${params.script}`) + '\n')
  }
}

interface ExecuteLifecycleParams {
  hookManifest: LifecyclePackage
  opts: RunLifecycleHookOptions
  optional: boolean
  scriptShell?: string
  shownScript: string
  stage: string
}

async function executeLifecycle (params: ExecuteLifecycleParams): Promise<void> {
  const { hookManifest, opts, optional, scriptShell, shownScript, stage } = params
  const logLevel = (opts.stdio !== 'inherit' || opts.silent) ? 'silent' : undefined

  await lifecycle(hookManifest, stage, opts.pkgRoot, {
    dir: opts.rootModulesDir,
    wdBinDir: opts.wdBinDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: {
      ...opts.extraEnv,
      INIT_CWD: opts.initCwd ?? process.cwd(),
      PNPM_SCRIPT_SRC_DIR: opts.pkgRoot,
      ...(opts.userAgent ? { npm_config_user_agent: opts.userAgent } : {}),
    },
    log: {
      clearProgress: noop,
      info: noop,
      level: logLevel,
      pause: noop,
      resume: noop,
      showProgress: noop,
      silly: (_prefix: string, _logId: string, type: string, line?: number) => handleLifecycleLog(type, line, { depPath: opts.depPath, optional, stage, stdio: opts.stdio, wd: opts.pkgRoot }),
      verbose: (_prefix: string, _logId: string, type: string, line?: number) => handleLifecycleLog(type, line, { depPath: opts.depPath, optional, stage, stdio: opts.stdio, wd: opts.pkgRoot }),
      warn: (...msg: string[]) => {
        globalWarn(msg.join(' '))
      },
    },
    onSpawn: trackChildProcess,
    raiseOnInterrupt: opts.raiseOnInterrupt,
    runConcurrently: true,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell,
    shellEmulator: opts.shellEmulator,
    shownScript,
    stdio: opts.stdio ?? 'pipe',
    unsafePerm: opts.unsafePerm,
  })
}

interface LogContext {
  depPath: string
  optional: boolean
  stage: string
  stdio?: 'inherit' | 'pipe'
  wd: string
}

function handleLifecycleLog (stdtype: string, line: number | undefined, ctx: LogContext): void {
  switch (stdtype) {
    case 'stdout':
    case 'stderr':
      lifecycleLogger.debug({
        depPath: ctx.depPath,
        line: (line ?? 0).toString(),
        stage: ctx.stage,
        stdio: stdtype,
        wd: ctx.wd,
      })
      return
    case 'Returned: code:': {
      if (ctx.stdio === 'inherit') {
        return
      }
      lifecycleLogger.debug({
        depPath: ctx.depPath,
        exitCode: line ?? 1,
        optional: ctx.optional,
        stage: ctx.stage,
        wd: ctx.wd,
      })
    }
  }
}

/**
 * Set `scripts.install` to `node-gyp rebuild` when `root` holds a binding.gyp.
 *
 * The caller decides whether the synthesized script applies: only when the
 * manifest declares no `install` or `preinstall` script and does not opt out
 * with `gypfile: false` (see `npm help scripts` and
 * https://docs.npmjs.com/cli/v12/configuring-npm/package-json#gypfile).
 */
function checkBindingGyp (
  root: string,
  scripts: PackageScripts
) {
  if (existsSync(path.join(root, 'binding.gyp'))) {
    scripts.install = 'node-gyp rebuild'
  }
}

function getId (manifest: ProjectManifest | DependencyManifest): string {
  return `${manifest.name ?? ''}@${manifest.version ?? ''}`
}

function isWindowsBatchFile (scriptShell: string) {
  // Node.js performs a similar check to determine whether it should throw
  // EINVAL when spawning a .cmd/.bat file.
  //
  // https://github.com/nodejs/node/commit/6627222409#diff-1e725bfa950eda4d4b5c0c00a2bb6be3e5b83d819872a1adf2ef87c658273903
  const scriptShellLower = scriptShell.toLowerCase()
  return isWindows() && (scriptShellLower.endsWith('.cmd') || scriptShellLower.endsWith('.bat'))
}
