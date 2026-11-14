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

type LifecycleStderrSink = (record: Record<string, unknown>) => void

const activeJsonLifecycleSinks = new Set<LifecycleStderrSink>()

function dispatchJsonLifecycleLog (log: LogBase): void {
  const record = log as unknown as Record<string, unknown>
  if (record.name !== 'pnpm:lifecycle') return
  for (const sink of activeJsonLifecycleSinks) {
    sink(record)
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
    stdio: opts.json ? 'pipe' : 'inherit',
    unsafePerm: opts.unsafePerm ?? false,
    userAgent: opts.userAgent,
  }
  if (!opts.json || opts.loglevel === 'silent') {
    await runLifecycleHook(stage, manifest, lifecycleOpts)
    return
  }
  await runPipedLifecycleHookToStderr(stage, manifest, lifecycleOpts)
}

/**
 * Runs a piped lifecycle hook and forwards its captured output lines to
 * `stderr`, since `--json` disables the CLI reporter to keep `stdout` pure.
 */
async function runPipedLifecycleHookToStderr (
  stage: 'preversion' | 'version' | 'postversion',
  manifest: ProjectManifest,
  lifecycleOpts: RunLifecycleHookOptions
): Promise<void> {
  const sink: LifecycleStderrSink = (record) => {
    if (
      record.depPath === lifecycleOpts.depPath &&
      record.stage === stage &&
      record.wd === lifecycleOpts.pkgRoot &&
      typeof record.line === 'string'
    ) {
      process.stderr.write(`${record.line}\n`)
    }
  }
  if (activeJsonLifecycleSinks.size === 0) {
    streamParser.on('data', dispatchJsonLifecycleLog)
  }
  activeJsonLifecycleSinks.add(sink)
  try {
    await runLifecycleHook(stage, manifest, lifecycleOpts)
  } finally {
    await new Promise<void>((resolve) => setImmediate(resolve))
    activeJsonLifecycleSinks.delete(sink)
    if (activeJsonLifecycleSinks.size === 0) {
      streamParser.removeListener('data', dispatchJsonLifecycleLog)
    }
  }
}
