import path from 'node:path'

import { binDirOf, type Config, createProjectModulesDirResolver } from '@pnpm/config.reader'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  makeProjectNodePathOption,
  type RunLifecycleHookOptions,
} from '@pnpm/exec.lifecycle'
import type { ProjectManifest } from '@pnpm/types'
import { realpathMissing } from 'realpath-missing'

import { existsInDir } from './existsInDir.js'
import type { RunOpts } from './run.js'

export async function createLifecycleOpts (
  opts: RunOpts,
  project: { dir: string, manifest: ProjectManifest, stdio: 'pipe' | 'inherit' }
): Promise<RunLifecycleHookOptions> {
  const { dir } = project
  const modulesDirFor = createProjectModulesDirResolver(opts)
  const wdBinDir = binDirOf(dir, modulesDirFor(project.manifest.name))
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: dir,
    wdBinDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir: path.dirname(wdBinDir), rootDir: dir }, opts) },
    pkgRoot: dir,
    rootModulesDir: await realpathMissing(path.dirname(wdBinDir)),
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    silent: suppressesScriptEcho(opts),
    shellEmulator: opts.shellEmulator,
    stdio: project.stdio,
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
    userAgent: opts.userAgent,
  }
  lifecycleOpts.extraEnv = addNodeLoaderOptions(opts, dir, lifecycleOpts.extraEnv)
  return lifecycleOpts
}

function addNodeLoaderOptions (
  opts: RunOpts,
  dir: string,
  extraEnv: RunLifecycleHookOptions['extraEnv']
): RunLifecycleHookOptions['extraEnv'] {
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const pnpPath = (opts.workspaceDir && existsPnp(opts.workspaceDir)) ?? existsPnp(dir)
  if (pnpPath) {
    extraEnv = {
      ...extraEnv,
      ...makeNodeRequireOption(pnpPath, extraEnv),
    }
  }
  const existsPackageMap = existsInDir.bind(null, path.join(opts.modulesDir ?? 'node_modules', '.package-map.json'))
  const packageMapPath = opts.nodeExperimentalPackageMap
    ? (opts.workspaceDir && existsPackageMap(opts.workspaceDir)) ?? existsPackageMap(dir)
    : undefined
  if (packageMapPath) {
    extraEnv = {
      ...extraEnv,
      ...makeNodePackageMapOption(packageMapPath, extraEnv),
    }
  }
  return extraEnv
}

/** The `$ <script>` echo is info-level output. */
export function suppressesScriptEcho (opts: Pick<Config, 'loglevel' | 'reporter'>): boolean {
  return opts.reporter === 'silent' || opts.loglevel === 'silent' || opts.loglevel === 'error' || opts.loglevel === 'warn'
}
