import type { StdioOptions } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import uidNumber from 'uid-number'

import { extendPath } from './extendPath.js'
import type { Callback, LifecycleError, LifecycleOptions, LifecyclePackage, ScriptRun } from './lifecycleTypes.js'
import { makeEnv, PATH } from './makeEnv.js'
import { makePackageManagerEnv } from './makePackageManagerEnv.js'
import { logId, runEmulated, runSpawned } from './runScript.js'
import { scriptBody, selectShell, useShellEmulator } from './selectShell.js'
import { reserveSignalRelay, spawnsInOwnProcessGroup } from './signals.js'
import { spawn } from './spawn.js'

export type { LifecycleError, LifecycleLog, LifecycleOptions, LifecyclePackage } from './lifecycleTypes.js'
export { makeEnv, type MakeEnvOptions } from './makeEnv.js'
export { makePackageManagerEnv } from './makePackageManagerEnv.js'
export { appendScriptArgs, showScriptWithArgs } from './scriptArgs.js'
export { commandParsedByCmd } from './selectShell.js'
export type { ProcessGroupWatchdog, RelaySignalsOptions, SignalRelay, SignalTarget } from './signals.js'
export { hasControllingTerminal, relaySignals, reserveSignalRelay, spawnsInOwnProcessGroup, waitForProcessGroup, watchProcessGroup } from './signals.js'
export type { LifecycleChildProcess } from './spawn.js'

const require = createRequire(import.meta.url)

let DEFAULT_NODE_GYP_PATH: string | undefined
try {
  DEFAULT_NODE_GYP_PATH = require.resolve('node-gyp/bin/node-gyp')
} catch {}

/**
 * The `node-gyp` wrappers put on a script's `PATH`. They sit beside `lib` in
 * the package; the pnpm CLI bundle flattens this module into its `dist`
 * directory, which ships its own copy of the wrappers next to the bundle.
 */
const NODE_GYP_BIN_DIR = [
  path.join(import.meta.dirname, 'node-gyp-bin'),
  path.join(import.meta.dirname, '..', 'node-gyp-bin'),
].find((dir) => fs.existsSync(dir)) ?? path.join(import.meta.dirname, '..', 'node-gyp-bin')

/** The `PATH` a script in `wd` runs with. */
export function scriptSearchPath (wd: string, opts: Pick<LifecycleOptions, 'wdBinDir' | 'extraBinPaths' | 'extraEnv'>): string {
  return extendPath(wd, opts.extraEnv?.[PATH] ?? process.env[PATH], { ...opts, nodeGypBinDir: NODE_GYP_BIN_DIR })
}

export function lifecycle (pkg: LifecyclePackage, stage: string, wd: string, opts: LifecycleOptions): Promise<void> {
  const relayReservation = opts.raiseOnInterrupt ? reserveSignalRelay() : undefined
  return new Promise<void>((resolve, reject) => {
    const finish = (err?: LifecycleError | null): void => {
      relayReservation?.release()
      if (err) reject(err)
      else resolve()
    }
    opts.log.info('lifecycle', logId(pkg, stage), pkg._id)
    dropIgnoredScripts(pkg, stage, opts)

    hookStat(opts.dir, stage, statError => {
      // makeEnv is a slow operation. This guard clause prevents makeEnv being called
      // and avoids a ton of unnecessary work, and results in a major perf boost.
      if (!pkg.scripts![stage] && statError) {
        finish()
        return
      }
      runLifecycleInValidWd({ pkg, stage, wd, opts, relayReservation }, finish)
    })
  })
}

function dropIgnoredScripts (pkg: LifecyclePackage, stage: string, opts: LifecycleOptions): void {
  if (!pkg.scripts) pkg.scripts = {}

  if (stage === 'prepublish' && opts.ignorePrepublish) {
    opts.log.info('lifecycle', logId(pkg, stage), 'ignored because ignore-prepublish is set to true', pkg._id)
    delete pkg.scripts.prepublish
  }
}

type LifecycleRun = Omit<ScriptRun, 'cmd' | 'env'>

function runLifecycleInValidWd (run: LifecycleRun, finish: Callback): void {
  validWd(run.wd, (er, wd) => {
    if (er) {
      finish(er)
      return
    }
    let env: Record<string, string>
    try {
      env = makeLifecycleEnv(run, wd)
    } catch (err: unknown) {
      finish(err as LifecycleError)
      return
    }
    runLifecycle({ pkg: run.pkg, stage: run.stage, wd, env, opts: run.opts, relayReservation: run.relayReservation }, finish)
  })
}

function makeLifecycleEnv (run: LifecycleRun, wd: string): Record<string, string> {
  const { pkg, stage, opts } = run
  const env = makeEnv(pkg, opts)
  env.npm_lifecycle_event = stage
  Object.assign(env, makePackageManagerEnv(env))
  env.npm_package_json = path.join(wd, 'package.json')
  if (!env.npm_config_node_gyp && DEFAULT_NODE_GYP_PATH) {
    env.npm_config_node_gyp = DEFAULT_NODE_GYP_PATH
  }
  if (opts.extraEnv) {
    for (const [key, value] of Object.entries(opts.extraEnv)) {
      env[key] = value
    }
  }

  // 'nobody' typically doesn't have permission to write to /tmp
  // even if it's never used, sh freaks out.
  if (!opts.unsafePerm) {
    const tmpdir = path.join(wd, 'node_modules', '.tmp')
    fs.mkdirSync(tmpdir, { recursive: true })
    env.TMPDIR = tmpdir
  }
  return env
}

const hookStatCache = new Map<string, NodeJS.ErrnoException | null>()

function hookStat (dir: string, stage: string, cb: (statError: NodeJS.ErrnoException | null) => void): void {
  const hook = path.join(dir, '.hooks', stage)
  const cachedStatError = hookStatCache.get(hook)

  if (cachedStatError === undefined) {
    fs.stat(hook, statError => {
      hookStatCache.set(hook, statError)
      cb(statError)
    })
    return
  }

  setImmediate(() => cb(cachedStatError))
}

function validWd (dir: string, cb: (err: Error | null, wd: string) => void): void {
  fs.stat(dir, (er, st) => {
    if (er || !st.isDirectory()) {
      const parentDir = path.dirname(dir)
      if (parentDir === dir) {
        cb(new Error('Could not find suitable wd'), dir)
        return
      }
      validWd(parentDir, cb)
      return
    }
    cb(null, dir)
  })
}

function runLifecycle (run: Omit<ScriptRun, 'cmd'>, cb: Callback): void {
  const { wd, env, opts } = run
  env[PATH] = extendPath(wd, env[PATH], { ...opts, nodeGypBinDir: NODE_GYP_BIN_DIR })

  const done: Callback = (er) => {
    cb(forgiveFailureIfAllowed(run, er))
  }
  const runHook = (): void => {
    runHookLifecycle(run, done)
  }
  if (!preparePackageScript(run)) {
    runHook()
    return
  }
  // run package lifecycle scripts in the package root, or the nearest parent.
  runCmd({ ...run, cmd: env.npm_lifecycle_script, shownCmd: opts.shownScript }, (er) => {
    if (er) {
      done(er)
      return
    }
    runHook()
  })
}

/** Whether the package has a script for the stage that is to run. */
function preparePackageScript (run: Omit<ScriptRun, 'cmd'>): boolean {
  const { pkg, stage, env, opts } = run
  if (opts.ignoreScripts) {
    opts.log.info('lifecycle', logId(pkg, stage), 'ignored because ignore-scripts is set to true', pkg._id)
    return false
  }
  if (pkg.scripts != null && Object.prototype.hasOwnProperty.call(pkg.scripts, stage)) {
    // define this here so it's available to all scripts.
    env.npm_lifecycle_script = pkg.scripts[stage]!
    return true
  }
  opts.log.silly('lifecycle', logId(pkg, stage), `no script for ${stage}, continuing`)
  return false
}

function forgiveFailureIfAllowed (run: Omit<ScriptRun, 'cmd'>, er?: LifecycleError | null): LifecycleError | null | undefined {
  if (!er) return er
  const { pkg, stage, opts } = run
  if (opts.force) {
    opts.log.info('lifecycle', logId(pkg, stage), 'forced, continuing', er)
    return null
  }
  if (opts.failOk) {
    opts.log.warn('lifecycle', logId(pkg, stage), 'continuing anyway', er.message)
    return null
  }
  return er
}

function runHookLifecycle (run: Omit<ScriptRun, 'cmd'>, cb: Callback): void {
  const { opts, stage } = run
  hookStat(opts.dir, stage, er => {
    if (er) {
      cb()
      return
    }
    runCmd({ ...run, cmd: path.join(opts.dir, '.hooks', stage) }, cb)
  })
}

let running = false
const queue: Array<[ScriptRun, Callback]> = []

function runCmd (run: ScriptRun, cb: Callback): void {
  const { pkg, stage, opts } = run
  if (opts.runConcurrently !== true) {
    if (running) {
      queue.push([run, cb])
      return
    }

    running = true
  }
  opts.log.pause()
  const unsafe = opts.unsafePerm || process.platform === 'win32'
  opts.log.verbose('lifecycle', logId(pkg, stage), 'unsafe-perm in lifecycle', opts.unsafePerm)

  const finish: Callback = (er) => {
    cb(er)
    opts.log.resume()
    process.nextTick(dequeue)
  }
  if (unsafe) {
    runCmdAs(run, null, finish)
  } else {
    uidNumber(opts.user, opts.group, (er, uid, gid) => {
      runCmdAs(run, { uid, gid }, finish)
    })
  }
}

function dequeue (): void {
  running = false
  const queued = queue.shift()
  if (queued) {
    runCmd(...queued)
  }
}

/** Run the script as `owner`, or as the current user when it is null. */
function runCmdAs (run: ScriptRun, owner: { uid: number, gid: number } | null, cb: Callback): void {
  const { cmd, pkg, stage, wd, env, opts } = run
  const ownProcessGroup = spawnsInOwnProcessGroup()
  const conf: {
    cwd: string
    detached: boolean
    env: Record<string, string>
    stdio: StdioOptions
    uid?: number
    gid?: number
    windowsVerbatimArguments?: boolean
  } = {
    cwd: wd,
    detached: ownProcessGroup,
    env,
    stdio: opts.stdio ?? [0, 1, 2],
  }

  if (owner) {
    conf.uid = owner.uid ^ 0
    conf.gid = owner.gid ^ 0
  }

  const scriptShell = opts.scriptShell || undefined
  const shell = selectShell(scriptShell, process.platform, process.env.comspec)
  if (shell.windowsVerbatimArguments) {
    conf.windowsVerbatimArguments = true
  }

  opts.log.verbose('lifecycle', logId(pkg, stage), 'PATH:', env[PATH])
  opts.log.verbose('lifecycle', logId(pkg, stage), 'CWD:', wd)
  opts.log.silly('lifecycle', logId(pkg, stage), 'Args:', [shell.shFlag, cmd])

  if (useShellEmulator(opts.shellEmulator, scriptShell)) {
    runEmulated(run, cb)
    return
  }
  const proc = spawn(shell.sh, [shell.shFlag, scriptBody(shell, cmd)], { ...conf, log: opts.log })
  runSpawned(run, { proc, ownProcessGroup }, cb)
}
