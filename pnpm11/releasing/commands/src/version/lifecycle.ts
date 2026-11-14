import path from 'node:path'

import { readProjectManifest } from '@pnpm/cli.utils'
import { binDirOf, type Config, type createProjectModulesDirResolver } from '@pnpm/config.reader'
import { makeProjectNodePathOption, runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { type LogBase, streamParser } from '@pnpm/logger'
import type { ProjectManifest } from '@pnpm/types'

export interface VersionChange {
  name: string
  currentVersion: string
  newVersion: string
  path: string
  manifestPath: string
}

export interface VersionLifecycleOptions extends Config {
  dryRun?: boolean
  json?: boolean
  modulesDirFor: ReturnType<typeof createProjectModulesDirResolver>
}

type LifecycleLineHandler = (line: string) => void

const activeJsonLifecycleSinks = new Map<string, Set<LifecycleLineHandler>>()

function lifecycleSinkKey (wd: string, stage: string, depPath: string): string {
  return `${wd}\0${stage}\0${depPath}`
}

function dispatchJsonLifecycleLog (log: LogBase): void {
  const record = log as unknown as Record<string, unknown>
  if (
    record.name !== 'pnpm:lifecycle' ||
    typeof record.wd !== 'string' ||
    typeof record.stage !== 'string' ||
    typeof record.depPath !== 'string' ||
    typeof record.line !== 'string'
  ) {
    return
  }
  const handlers = activeJsonLifecycleSinks.get(lifecycleSinkKey(record.wd, record.stage, record.depPath))
  if (!handlers) return
  for (const handler of handlers) {
    handler(record.line)
  }
}

function registerLifecycleSink (key: string, handler: LifecycleLineHandler): void {
  if (activeJsonLifecycleSinks.size === 0) {
    streamParser.on('data', dispatchJsonLifecycleLog)
  }
  let handlers = activeJsonLifecycleSinks.get(key)
  if (!handlers) {
    handlers = new Set()
    activeJsonLifecycleSinks.set(key, handlers)
  }
  handlers.add(handler)
}

function unregisterLifecycleSink (key: string, handler: LifecycleLineHandler): void {
  const handlers = activeJsonLifecycleSinks.get(key)
  if (handlers) {
    handlers.delete(handler)
    if (handlers.size === 0) {
      activeJsonLifecycleSinks.delete(key)
    }
  }
  if (activeJsonLifecycleSinks.size === 0) {
    streamParser.removeListener('data', dispatchJsonLifecycleLog)
  }
}

export async function runVersionLifecycleHook (
  stage: 'preversion' | 'version' | 'postversion',
  change: VersionChange,
  opts: VersionLifecycleOptions
): Promise<void> {
  if (opts.ignoreScripts === true || opts.dryRun) return

  const { manifest } = await readProjectManifest(change.path)
  const wdBinDir = binDirOf(change.path, opts.modulesDirFor(manifest.name))
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: change.name,
    wdBinDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir: path.dirname(wdBinDir), rootDir: change.path }, opts) },
    initCwd: opts.dir,
    pkgRoot: change.path,
    rootModulesDir: path.dirname(wdBinDir),
    scriptShell: opts.scriptShell,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    shellEmulator: opts.shellEmulator,
    stdio: opts.json ? ['inherit', 'pipe', 'pipe'] : 'inherit',
    unsafePerm: opts.unsafePerm ?? false,
    userAgent: opts.userAgent,
  }
  if (!opts.json || opts.loglevel === 'silent') {
    await runLifecycleHook(stage, manifest, lifecycleOpts)
    return
  }
  const reportOnlyOnFailure = opts.loglevel === 'warn' || opts.loglevel === 'error'
  await runPipedLifecycleHookToStderr(stage, manifest, lifecycleOpts, reportOnlyOnFailure)
}

/**
 * Runs a piped lifecycle hook and forwards its captured output lines to
 * `stderr`, since `--json` disables the CLI reporter to keep `stdout` pure.
 * At `warn` and `error` log levels, output is buffered and written only when
 * the hook fails.
 */
async function runPipedLifecycleHookToStderr (
  stage: 'preversion' | 'version' | 'postversion',
  manifest: ProjectManifest,
  lifecycleOpts: RunLifecycleHookOptions,
  reportOnlyOnFailure: boolean
): Promise<void> {
  const bufferedLines: string[] = []
  const handler: LifecycleLineHandler = reportOnlyOnFailure
    ? (line) => {
      bufferedLines.push(line)
    }
    : (line) => {
      process.stderr.write(`${line}\n`)
    }
  const key = lifecycleSinkKey(lifecycleOpts.pkgRoot, stage, lifecycleOpts.depPath)
  registerLifecycleSink(key, handler)
  let failed = false
  try {
    await runLifecycleHook(stage, manifest, lifecycleOpts)
  } catch (err: unknown) {
    failed = true
    throw err
  } finally {
    await new Promise<void>((resolve) => setImmediate(resolve))
    unregisterLifecycleSink(key, handler)
    if (failed && bufferedLines.length > 0) {
      process.stderr.write(`${bufferedLines.join('\n')}\n`)
    }
  }
}
