import { lifecycleLogger } from '@pnpm/core-loggers'
import { globalWarn } from '@pnpm/logger'
import lifecycle from '@pnpm/npm-lifecycle'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { existsSync } from 'node:fs'

function noop() {} // eslint-disable-line:no-empty

export interface RunLifecycleHookOptions {
  args?: string[] | undefined
  depPath: string
  extraBinPaths?: string[] | undefined
  extraEnv?: Record<string, string> | undefined
  initCwd?: string | undefined
  optional?: boolean | undefined
  pkgRoot: string
  rawConfig: object
  rootModulesDir: string
  scriptShell?: string | undefined
  silent?: boolean | undefined
  scriptsPrependNodePath?: boolean | 'warn-only' | undefined
  shellEmulator?: boolean | undefined
  stdio?: string | undefined
  unsafePerm: boolean
}

export async function runLifecycleHook(
  stage: string,
  manifest: ProjectManifest | DependencyManifest,
  opts: RunLifecycleHookOptions
): Promise<void> {
  const optional = opts.optional === true

  const m = { _id: getId(manifest), ...manifest }
  m.scripts = { ...m.scripts }

  if (stage === 'start' && !m.scripts.start) {
    if (!existsSync('server.js')) {
      throw new PnpmError(
        'NO_SCRIPT_OR_SERVER',
        'Missing script start or file server.js'
      )
    }
    m.scripts.start = 'node server.js'
  }
  if (opts.args?.length && m.scripts?.[stage]) {
    const escapedArgs = opts.args.map((arg) => JSON.stringify(arg))
    m.scripts[stage] = `${m.scripts[stage]} ${escapedArgs.join(' ')}`
  }
  // This script is used to prevent the usage of npm or Yarn.
  // It does nothing, when pnpm is used, so we may skip its execution.
  if (m.scripts[stage] === 'npx only-allow pnpm') return
  if (opts.stdio !== 'inherit') {
    lifecycleLogger.debug({
      depPath: opts.depPath,
      optional,
      script: m.scripts[stage],
      stage,
      wd: opts.pkgRoot,
    })
  }
  const logLevel =
    opts.stdio !== 'inherit' || opts.silent ? 'silent' : undefined
  await lifecycle(m, stage, opts.pkgRoot, {
    config: {
      ...opts.rawConfig,
      'frozen-lockfile': false,
    },
    dir: opts.rootModulesDir,
    extraBinPaths: opts.extraBinPaths ?? [],
    extraEnv: {
      ...opts.extraEnv,
      INIT_CWD: opts.initCwd ?? process.cwd(),
      PNPM_SCRIPT_SRC_DIR: opts.pkgRoot,
    },
    log: {
      clearProgress: noop,
      info: noop,
      level: logLevel,
      pause: noop,
      resume: noop,
      showProgress: noop,
      silly: npmLog,
      verbose: npmLog,
      warn: (...msg: string[]) => {
        globalWarn(msg.join(' '))
      },
    },
    runConcurrently: true,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    stdio: opts.stdio ?? 'pipe',
    unsafePerm: opts.unsafePerm,
  })

  function npmLog(
    _prefix: string,
    _logId: string,
    stdtype: string,
    line: string
  ): void {
    switch (stdtype) {
      case 'stdout':
      case 'stderr':
        lifecycleLogger.debug({
          depPath: opts.depPath,
          line: line.toString(),
          stage,
          stdio: stdtype,
          wd: opts.pkgRoot,
        })
        return
      case 'Returned: code:': {
        if (opts.stdio === 'inherit') {
          // Preventing the pnpm reporter from overriding the project's script output
          return
        }
        const code = arguments[3] ?? 1
        lifecycleLogger.debug({
          depPath: opts.depPath,
          exitCode: code,
          optional,
          stage,
          wd: opts.pkgRoot,
        })
      }
    }
  }
}

function getId(manifest: ProjectManifest | DependencyManifest): string {
  return `${manifest.name ?? ''}@${manifest.version ?? ''}`
}
