import { lifecycleLogger } from '@pnpm/core-loggers'
import { DependencyManifest, ProjectManifest } from '@pnpm/types'
import lifecycle = require('@zkochan/npm-lifecycle')

function noop () {} // eslint-disable-line:no-empty

export interface RunLifecycleHookOptions {
  args?: string[]
  depPath: string
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  initCwd?: string
  optional?: boolean
  pkgRoot: string
  rawConfig: object
  rootModulesDir: string
  scriptShell?: string
  silent?: boolean
  shellEmulator?: boolean
  stdio?: string
  unsafePerm: boolean
}

export default async function runLifecycleHook (
  stage: string,
  manifest: ProjectManifest | DependencyManifest,
  opts: RunLifecycleHookOptions
) {
  const optional = opts.optional === true

  const m = { _id: getId(manifest), ...manifest }
  m.scripts = { ...m.scripts }

  if (stage === 'start' && !m.scripts.start) {
    m.scripts.start = 'node server.js'
  }
  if (opts.args?.length && m.scripts?.[stage]) {
    m.scripts[stage] = `${m.scripts[stage]} ${opts.args.map((arg) => `"${arg}"`).join(' ')}`
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
  const logLevel = (opts.stdio !== 'inherit' || opts.silent)
    ? 'silent' : undefined
  await lifecycle(m, stage, opts.pkgRoot, {
    config: opts.rawConfig,
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
      warn: noop,
    },
    runConcurrently: true,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    stdio: opts.stdio ?? 'pipe',
    unsafePerm: opts.unsafePerm,
  })

  function npmLog (prefix: string, logid: string, stdtype: string, line: string) {
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
      const code = arguments[3]
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

function getId (manifest: ProjectManifest | DependencyManifest) {
  return `${manifest.name ?? ''}@${manifest.version ?? ''}`
}
