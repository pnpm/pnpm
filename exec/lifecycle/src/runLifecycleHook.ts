import { lifecycleLogger } from '@pnpm/core-loggers'
import { globalWarn } from '@pnpm/logger'
import lifecycle from '@pnpm/npm-lifecycle'
import { type DependencyManifest, type ProjectManifest, type PrepareExecutionEnv } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { existsSync } from 'fs'
import isWindows from 'is-windows'

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
  scriptsPrependNodePath?: boolean | 'warn-only'
  shellEmulator?: boolean
  stdio?: string
  unsafePerm: boolean
  prepareExecutionEnv?: PrepareExecutionEnv
}

export async function runLifecycleHook (
  stage: string,
  manifest: ProjectManifest | DependencyManifest,
  opts: RunLifecycleHookOptions
): Promise<void> {
  const optional = opts.optional === true

  // To remediate CVE_2024_27980, Node.js does not allow .bat or .cmd files to
  // be spawned without the "shell: true" option.
  //
  // https://nodejs.org/api/child_process.html#spawning-bat-and-cmd-files-on-windows
  //
  // Unfortunately, setting spawn's shell option also causes arguments to be
  // evaluated before they're passed to the shell, resulting in a surprising
  // behavior difference only with .bat/.cmd files.
  //
  // Instead of showing a "spawn EINVAL" error, let's throw a clearer error that
  // this isn't supported.
  //
  // If this behavior needs to be supported in the future, the arguments would
  // need to be escaped before they're passed to the .bat/.cmd file. For
  // example, scripts such as "echo %PATH%" should be passed verbatim rather
  // than expanded. This is difficult to do correctly. Other open source tools
  // (e.g. Rust) attempted and introduced bugs. The Rust blog has a good
  // high-level explanation of the same security vulnerability Node.js patched.
  //
  // https://blog.rust-lang.org/2024/04/09/cve-2024-24576.html#overview
  //
  // Note that npm (as of version 10.5.0) doesn't support setting script-shell
  // to a .bat or .cmd file either.
  if (opts.scriptShell != null && isWindowsBatchFile(opts.scriptShell)) {
    throw new PnpmError('ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS', 'Cannot spawn .bat or .cmd as a script shell.', {
      hint: `\
The .npmrc script-shell option was configured to a .bat or .cmd file. These cannot be used as a script shell reliably.

Please unset the script-shell option, or configure it to a .exe instead.
`,
    })
  }

  const m = { _id: getId(manifest), ...manifest }
  m.scripts = { ...m.scripts }

  if (stage === 'start' && !m.scripts.start) {
    if (!existsSync('server.js')) {
      throw new PnpmError('NO_SCRIPT_OR_SERVER', 'Missing script start or file server.js')
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
  const logLevel = (opts.stdio !== 'inherit' || opts.silent)
    ? 'silent'
    : undefined
  const extraBinPaths = (await opts.prepareExecutionEnv?.({
    extraBinPaths: opts.extraBinPaths,
    executionEnv: (manifest as ProjectManifest).pnpm?.executionEnv,
  }))?.extraBinPaths ?? opts.extraBinPaths
  await lifecycle(m, stage, opts.pkgRoot, {
    config: {
      ...opts.rawConfig,
      'frozen-lockfile': false,
    },
    dir: opts.rootModulesDir,
    extraBinPaths,
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

  function npmLog (prefix: string, logId: string, stdtype: string, line: string): void {
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
