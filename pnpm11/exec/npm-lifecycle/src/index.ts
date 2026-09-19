import type { StdioOptions } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { PassThrough } from 'node:stream'

import byline from '@pnpm/byline'
import { PnpmError } from '@pnpm/error'
import { npath } from '@yarnpkg/fslib'
import { execute } from '@yarnpkg/shell'
import uidNumber from 'uid-number'

import { extendPath } from './extendPath.js'
import { type LifecycleChildProcess, spawn } from './spawn.js'

export type { LifecycleChildProcess } from './spawn.js'

export interface LifecycleLog {
  info (...args: unknown[]): void
  warn (...args: unknown[]): void
  silly (...args: unknown[]): void
  verbose (...args: unknown[]): void
  pause (): void
  resume (): void
  level?: string
  progressEnabled?: boolean
  disableProgress?: () => void
  enableProgress?: () => void
  clearProgress?: () => void
  showProgress?: () => void
}

export interface LifecyclePackage {
  _id?: string
  name?: string
  version?: string
  scripts?: Record<string, string | undefined>
  [field: string]: unknown
}

export interface LifecycleOptions {
  /** The `node_modules` directory whose `.hooks/<stage>` hook runs after the script. */
  dir: string
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  failOk?: boolean
  force?: boolean
  group?: string
  ignorePrepublish?: boolean
  ignoreScripts?: boolean
  log: LifecycleLog
  nodeOptions?: string
  onSpawn?: (child: LifecycleChildProcess) => void
  production?: boolean
  runConcurrently?: boolean
  scriptShell?: string
  scriptsPrependNodePath?: boolean | 'warn-only'
  shellEmulator?: boolean
  stdio?: StdioOptions
  unsafePerm?: boolean
  user?: string
}

export interface MakeEnvOptions {
  nodeOptions?: string
  production?: boolean
}

/** The failure of a lifecycle script, with its `code` set to `ELIFECYCLE`. */
export interface LifecycleError extends Error {
  code?: string
  errno?: number | string
  pkgid?: string
  pkgname?: string
  script?: string
  stage?: string
}

type Callback = (err?: LifecycleError | null) => void

/** One script or hook of one package, as the runner threads it through. */
interface ScriptRun {
  cmd: string
  pkg: LifecyclePackage
  stage: string
  wd: string
  env: Record<string, string>
  opts: LifecycleOptions
}

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

let PATH = 'PATH'

// windows calls it's path 'Path' usually, but this is not guaranteed.
if (process.platform === 'win32') {
  PATH = 'Path'
  Object.keys(process.env).forEach(e => {
    if (e.match(/^PATH$/i)) {
      PATH = e
    }
  })
}

export function lifecycle (pkg: LifecyclePackage, stage: string, wd: string, opts: LifecycleOptions): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    opts.log.info('lifecycle', logId(pkg, stage), pkg._id)
    if (!pkg.scripts) pkg.scripts = {}

    if (stage === 'prepublish' && opts.ignorePrepublish) {
      opts.log.info('lifecycle', logId(pkg, stage), 'ignored because ignore-prepublish is set to true', pkg._id)
      delete pkg.scripts.prepublish
    }

    hookStat(opts.dir, stage, statError => {
      // makeEnv is a slow operation. This guard clause prevents makeEnv being called
      // and avoids a ton of unnecessary work, and results in a major perf boost.
      if (!pkg.scripts![stage] && statError) {
        resolve()
        return
      }

      validWd(wd, (er, wd) => {
        if (er) {
          reject(er)
          return
        }

        const env = makeEnv(pkg, opts)
        env.npm_lifecycle_event = stage
        env.npm_node_execpath = env.NODE = env.NODE || process.execPath
        env.npm_package_json = path.join(wd, 'package.json')
        if ((process as { pkg?: unknown }).pkg != null) {
          // If the pnpm CLI was bundled by vercel/pkg then we cannot use the js path for npm_execpath
          // because in that case the js is in a virtual filesystem inside the executor.
          // Instead, we use the path to the exe file.
          env.npm_execpath = process.execPath
        } else {
          env.npm_execpath = process.argv[1] || process.cwd()
        }
        env.INIT_CWD = process.cwd()
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

        runLifecycle({ pkg, stage, wd, env, opts }, (er) => {
          if (er) {
            reject(er)
            return
          }
          resolve()
        })
      })
    })
  })
}

function logId (pkg: LifecyclePackage, stage: string): string {
  return `${pkg._id}~${stage}:`
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

function validWd (d: string, cb: (err: Error | null, wd: string) => void): void {
  fs.stat(d, (er, st) => {
    if (er || !st.isDirectory()) {
      const p = path.dirname(d)
      if (p === d) {
        cb(new Error('Could not find suitable wd'), d)
        return
      }
      validWd(p, cb)
      return
    }
    cb(null, d)
  })
}

function runLifecycle (run: Omit<ScriptRun, 'cmd'>, cb: Callback): void {
  const { pkg, stage, wd, env, opts } = run
  env[PATH] = extendPath(wd, env[PATH], { ...opts, nodeGypBinDir: NODE_GYP_BIN_DIR })

  let packageLifecycle = pkg.scripts != null && Object.prototype.hasOwnProperty.call(pkg.scripts, stage)

  if (opts.ignoreScripts) {
    opts.log.info('lifecycle', logId(pkg, stage), 'ignored because ignore-scripts is set to true', pkg._id)
    packageLifecycle = false
  } else if (packageLifecycle) {
    // define this here so it's available to all scripts.
    env.npm_lifecycle_script = pkg.scripts![stage]!
  } else {
    opts.log.silly('lifecycle', logId(pkg, stage), `no script for ${stage}, continuing`)
  }

  const tasks: Array<(next: Callback) => void> = []
  if (packageLifecycle) {
    // run package lifecycle scripts in the package root, or the nearest parent.
    tasks.push((next) => {
      runCmd({ ...run, cmd: env.npm_lifecycle_script }, next)
    })
  }
  tasks.push((next) => {
    runHookLifecycle(run, next)
  })

  let i = 0
  function next (er?: LifecycleError | null): void {
    if (er) {
      done(er)
      return
    }
    const task = tasks[i++]
    if (task) {
      task(next)
      return
    }
    done()
  }
  function done (er?: LifecycleError | null): void {
    if (er) {
      if (opts.force) {
        opts.log.info('lifecycle', logId(pkg, stage), 'forced, continuing', er)
        er = null
      } else if (opts.failOk) {
        opts.log.warn('lifecycle', logId(pkg, stage), 'continuing anyway', er.message)
        er = null
      }
    }
    cb(er)
  }
  next()
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
  const conf: {
    cwd: string
    env: Record<string, string>
    stdio: StdioOptions
    uid?: number
    gid?: number
    windowsVerbatimArguments?: boolean
  } = {
    cwd: wd,
    env,
    stdio: opts.stdio ?? [0, 1, 2],
  }

  if (owner) {
    conf.uid = owner.uid ^ 0
    conf.gid = owner.gid ^ 0
  }

  let sh = 'sh'
  let shFlag = '-c'

  const customShell = opts.scriptShell

  if (customShell) {
    sh = customShell
  } else if (process.platform === 'win32') {
    sh = process.env.comspec ?? 'cmd'
    shFlag = '/d /s /c'
    conf.windowsVerbatimArguments = true
  }

  opts.log.verbose('lifecycle', logId(pkg, stage), 'PATH:', env[PATH])
  opts.log.verbose('lifecycle', logId(pkg, stage), 'CWD:', wd)
  opts.log.silly('lifecycle', logId(pkg, stage), 'Args:', [shFlag, cmd])

  if (opts.shellEmulator) {
    runEmulated(run, cb)
    return
  }
  runSpawned(run, spawn(sh, [shFlag, cmd], { ...conf, log: opts.log }), cb)
}

function runEmulated (run: ScriptRun, cb: Callback): void {
  const { cmd, pkg, stage, wd, env, opts } = run
  const execOpts: Parameters<typeof execute>[2] = { cwd: npath.toPortablePath(wd), env }
  if (opts.stdio === 'pipe') {
    const stdout = new PassThrough()
    const stderr = new PassThrough()
    byline(stdout).on('data', (data: Buffer) => {
      opts.log.verbose('lifecycle', logId(pkg, stage), 'stdout', data.toString())
    })
    byline(stderr).on('data', (data: Buffer) => {
      opts.log.verbose('lifecycle', logId(pkg, stage), 'stderr', data.toString())
    })
    execOpts.stdout = stdout
    execOpts.stderr = stderr
  }
  const procError = createProcError(run, cb)
  execute(cmd, [], execOpts)
    .then((code) => {
      opts.log.silly('lifecycle', logId(pkg, stage), 'Returned: code:', code)
      let er: LifecycleError | undefined
      if (code) {
        er = new Error(`Exit status ${code}`)
        er.errno = code
      }
      procError(er)
    })
    .catch((err: LifecycleError) => procError(err))
}

function runSpawned (run: ScriptRun, proc: LifecycleChildProcess, cb: Callback): void {
  const { pkg, stage, opts } = run
  let spawnObserverFailed = false
  let spawnObserverError: LifecycleError | undefined

  const procError = createProcError(run, (er) => {
    process.removeListener('SIGTERM', procKill)
    process.removeListener('SIGINT', procKill)
    process.removeListener('SIGINT', procInterrupt)
    process.removeListener('exit', procKill)
    cb(er)
  })

  proc.on('error', (err: LifecycleError) => {
    procError(spawnObserverFailed ? spawnObserverError : err)
  })
  proc.on('close', (code: number | null, signal: NodeJS.Signals | null) => {
    opts.log.silly('lifecycle', logId(pkg, stage), 'Returned: code:', code, ' signal:', signal)
    let err: LifecycleError | undefined
    if (spawnObserverFailed) {
      err = spawnObserverError
    } else if (signal) {
      err = new PnpmError('CHILD_PROCESS_FAILED', `Command failed with signal "${signal}"`)
      process.kill(process.pid, signal)
    } else if (code) {
      err = new PnpmError('CHILD_PROCESS_FAILED', `Exit status ${code}`)
      err.errno = code
    }
    procError(err)
  })
  // Inherited streams are null on the child; only piped output is reported.
  if (proc.stdout) {
    byline(proc.stdout).on('data', (data: Buffer) => {
      opts.log.verbose('lifecycle', logId(pkg, stage), 'stdout', data.toString())
    })
  }
  if (proc.stderr) {
    byline(proc.stderr).on('data', (data: Buffer) => {
      opts.log.verbose('lifecycle', logId(pkg, stage), 'stderr', data.toString())
    })
  }
  process.once('SIGTERM', procKill)
  process.once('SIGINT', procInterrupt)
  process.on('exit', procKill)

  try {
    opts.onSpawn?.(proc)
  } catch (err: unknown) {
    spawnObserverFailed = true
    spawnObserverError = err as LifecycleError
    proc.kill()
  }

  let called = false
  function procKill (): void {
    if (called) return
    called = true
    proc.kill()
  }
  function procInterrupt (): void {
    if (!hasControllingTerminal()) {
      proc.kill('SIGINT')
    }
    process.once('SIGINT', procKill)
  }
}

/**
 * Wraps `cb` so that it runs once, with the failure of the script decorated
 * with the script's identity and the `ELIFECYCLE` code.
 */
function createProcError (run: ScriptRun, cb: Callback): (er?: LifecycleError | null) => void {
  const { cmd, pkg, stage, opts } = run
  let completed = false
  return (er) => {
    if (completed) return
    completed = true
    if (er) {
      opts.log.info('lifecycle', logId(pkg, stage), `Failed to exec ${stage} script`)
      er.message = `${pkg._id} ${stage}: \`${cmd}\`\n${er.message}`
      if (er.code !== 'EPERM') {
        er.code = 'ELIFECYCLE'
      }
      fs.stat(opts.dir, (statError) => {
        if (statError?.code === 'ENOENT' && opts.dir.split(path.sep).slice(-1)[0] === 'node_modules') {
          opts.log.warn('', 'Local package.json exists, but node_modules missing, did you mean to install?')
        }
      })
      er.pkgid = pkg._id
      er.stage = stage
      er.script = cmd
      er.pkgname = pkg.name
    }
    cb(er)
  }
}

/**
 * Whether this process has a controlling terminal.
 *
 * Ctrl+C there interrupts the whole foreground process group at once, and the
 * child runs in that group, so it has the SIGINT already. Relaying it would
 * deliver a second one, which ends a child that handled the first and then
 * left the default action in place (https://github.com/pnpm/pnpm/issues/7374).
 * Node.js cannot ask who sent a signal, so the terminal stands in for it:
 * without one, only kill() can reach the process, and the child needs the relay.
 */
function hasControllingTerminal (): boolean {
  if (process.platform === 'win32') return false
  let tty: number
  try {
    tty = fs.openSync('/dev/tty', fs.constants.O_RDONLY | fs.constants.O_NOCTTY)
  } catch {
    return false
  }
  fs.closeSync(tty)
  return true
}

export function makeEnv (data: Record<string, unknown>, opts: MakeEnvOptions, prefix?: string | null, env?: Record<string, string>): Record<string, string> {
  prefix = prefix ?? 'npm_package_'
  if (!env) {
    env = {}
    for (const i in process.env) {
      // npm_package_* are regenerated below. (npm|pnpm)_config_* auth settings
      // (e.g. _auth, _authToken, _password, //registry/:_authToken) are
      // stripped so they never leak into dependency lifecycle scripts. This
      // mirrors npm's own env-export filter, where config keys starting with
      // _, /, or @ (or containing :_) are treated as private. npm reads the
      // variables case-insensitively, so the filter does too.
      if (
        !i.match(/^npm_package_/) &&
        !i.match(/^(?:npm|pnpm)_config_(?:[/@_]|.*:_)/i) &&
        (!i.match(/^PATH$/i) || i === PATH)
      ) {
        env[i] = process.env[i]!
      }
    }

    // express and others respect the NODE_ENV value.
    if (opts.production) env.NODE_ENV = 'production'
  } else if (!Object.prototype.hasOwnProperty.call(data, '_lifecycleEnv')) {
    Object.defineProperty(data, '_lifecycleEnv',
      {
        value: env,
        enumerable: false,
      }
    )
  }

  if (opts.nodeOptions) env.NODE_OPTIONS = opts.nodeOptions

  for (const i in data) {
    if (i.charAt(0) !== '_') {
      const envKey = (prefix + i).replace(/\W/g, '_')
      if (
        !['name', 'version', 'config', 'engines', 'bin'].includes(i) &&
        !prefix.startsWith('npm_package_config_') &&
        !prefix.startsWith('npm_package_engines_') &&
        !prefix.startsWith('npm_package_bin_')
      ) {
        continue
      }
      const value = data[i]
      if (value && typeof value === 'object') {
        try {
          // quick and dirty detection for cyclical structures
          JSON.stringify(value)
          makeEnv(value as Record<string, unknown>, opts, `${envKey}_`, env)
        } catch {
          // usually these are package objects.
          // just get the path and basic details.
          const d = value as { name?: unknown, version?: unknown, path?: unknown }
          makeEnv(
            { name: d.name, version: d.version, path: d.path },
            opts,
            `${envKey}_`,
            env
          )
        }
      } else {
        env[envKey] = String(value)
        env[envKey] = env[envKey].includes('\n')
          ? JSON.stringify(env[envKey])
          : env[envKey]
      }
    }
  }

  return env
}
