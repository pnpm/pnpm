import type { StdioOptions } from 'node:child_process'

import type { SignalRelayReservation } from './signals.js'
import type { LifecycleChildProcess } from './spawn.js'

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
  /** The `.bin` holding `wd`'s own executables, in place of `<wd>/node_modules/.bin`. */
  wdBinDir?: string
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
  raiseOnInterrupt?: boolean
  runConcurrently?: boolean
  scriptShell?: string
  /** The package script as error messages show it, when that differs from what runs. */
  shownScript?: string
  scriptsPrependNodePath?: boolean | 'warn-only'
  shellEmulator?: boolean
  stdio?: StdioOptions
  unsafePerm?: boolean
  user?: string
}

/** The failure of a lifecycle script, with its `code` set to `ELIFECYCLE`. */
export interface LifecycleError extends Error {
  code?: string
  errno?: number | string
  pkgid?: string
  pkgname?: string
  script?: string
  /** The signal that killed the script, if one did. */
  signal?: NodeJS.Signals
  stage?: string
}

export type Callback = (err?: LifecycleError | null) => void

/** One script or hook of one package, as the runner threads it through. */
export interface ScriptRun {
  cmd: string
  /** `cmd` as error messages show it. */
  shownCmd?: string
  relayReservation?: SignalRelayReservation
  pkg: LifecyclePackage
  stage: string
  wd: string
  env: Record<string, string>
  opts: LifecycleOptions
}
