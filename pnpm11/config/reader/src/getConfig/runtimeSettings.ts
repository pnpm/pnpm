import path from 'node:path'

import { addEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import isWindows from 'is-windows'
import { pathAbsolute } from 'path-absolute'

import { binDirOf } from '../binDir.js'
import { getWorkspaceConcurrency } from '../concurrency.js'
import { getCacheDir, getStateDir } from '../dirs.js'
import { createProjectModulesDirResolver, getModulesDirsByProjectName } from '../projectConfig.js'
import type { ConfigBuildState, PnpmConfigInProgress } from './configBuildState.js'
import { getProcessEnv } from './envVars.js'
import { resolveSideEffectsCache } from './sideEffectsCache.js'

/**
 * Resolves the directories, the child process environment, and the settings
 * that several spellings or sources fold into one.
 */
export function applyRuntimeSettings (state: ConfigBuildState): void {
  const { pnpmConfig } = state
  resolveBinPaths(state)
  pnpmConfig.extraEnv = createExtraEnv(pnpmConfig)

  applyCacheAndStateDirDefaults(pnpmConfig)
  if (typeof pnpmConfig['color'] === 'boolean') {
    pnpmConfig.color = pnpmConfig['color'] ? 'always' : 'never'
  }
  // `catalogPrune`'s former name, still accepted. The canonical key wins
  // when both are set.
  pnpmConfig.catalogPrune ??= pnpmConfig.cleanupUnusedCatalogs
  // Every layer folds `virtualStoreType` into the boolean the rest of pnpm
  // reads, so the canonical spelling is restored here from the folded value
  // rather than from any one layer — otherwise `pnpm config get
  // virtualStoreType` could name the store a later layer overrode.
  if (state.explicitlySetKeys.has('enableGlobalVirtualStore')) {
    pnpmConfig.virtualStoreType = pnpmConfig.enableGlobalVirtualStore ? 'global' : 'project'
    state.explicitlySetKeys.add('virtualStoreType')
  }
  resolveProxySettings(pnpmConfig)
  applyNodeLinkerDefaults(pnpmConfig)
  if (!pnpmConfig.userConfig) {
    pnpmConfig.userConfig = state.npmrcResult.userConfig as Record<string, string>
  }
  resolveSideEffectsCache(pnpmConfig)

  pnpmConfig.workspaceConcurrency = getWorkspaceConcurrency(pnpmConfig.workspaceConcurrency)

  resolveDependencyTypes(pnpmConfig)

  if (pnpmConfig.ci && pnpmConfig.enableGlobalVirtualStore == null) {
    // Using a global virtual store in CI makes little sense,
    // as there is usually no warm cache in that environment.
    // However, if the user explicitly enabled GVS (e.g., for Nix builds
    // or CI systems with persistent caches), respect that setting.
    pnpmConfig.enableGlobalVirtualStore = false
  }

  extendNodePathForGlobalVirtualStore(pnpmConfig, state.env)
}

function applyCacheAndStateDirDefaults (pnpmConfig: PnpmConfigInProgress): void {
  if (!pnpmConfig.cacheDir) {
    pnpmConfig.cacheDir = getCacheDir(process)
  }
  if (!pnpmConfig.stateDir) {
    pnpmConfig.stateDir = getStateDir(process)
  }
}

function resolveBinPaths ({ cliOptions, pnpmConfig }: ConfigBuildState): void {
  // Derived once `modulesDir` is known, which the workspace manifest supplies
  // after the global branch above. Gated on the same `cliOptions['global']`
  // that branch is, not on the merged `global` setting: only `--global` sets
  // `bin` to the global directory, so a `global` that arrived from the
  // environment still needs the local one.
  if (!cliOptions['global'] && !pnpmConfig.bin) {
    pnpmConfig.bin = binDirOf(pnpmConfig.dir, pnpmConfig.modulesDir)
  }
  if (pnpmConfig.workspaceDir) {
    // The workspace root is a project like any other, so its own
    // `packageConfigs` entry moves the executables every member reaches
    // through these paths. Its manifest was read above.
    pnpmConfig.extraBinPaths = [binDirOf(
      pnpmConfig.workspaceDir,
      createProjectModulesDirResolver(pnpmConfig)(pnpmConfig.rootProjectManifest?.name)
    )]
  } else {
    pnpmConfig.extraBinPaths = []
  }
  pnpmConfig.modulesDirsByProjectName = getModulesDirsByProjectName(pnpmConfig)
}

function createExtraEnv (pnpmConfig: PnpmConfigInProgress): Record<string, string> {
  const extraEnv: Record<string, string> = {
    pnpm_config_verify_deps_before_run: 'false',
  }
  if (pnpmConfig.preferSymlinkedExecutables && !isWindows()) {
    const cwd = pnpmConfig.lockfileDir ?? pnpmConfig.dir
    extraEnv['NODE_PATH'] = pathAbsolute(path.join(getVirtualStoreDir(pnpmConfig), 'node_modules'), cwd)
  }
  return extraEnv
}

function getVirtualStoreDir (pnpmConfig: PnpmConfigInProgress): string {
  if (pnpmConfig.virtualStoreDir) return pnpmConfig.virtualStoreDir
  if (pnpmConfig.modulesDir) return path.join(pnpmConfig.modulesDir, '.pnpm')
  return 'node_modules/.pnpm'
}

function resolveProxySettings (pnpmConfig: PnpmConfigInProgress): void {
  if (!pnpmConfig.httpsProxy) {
    // An empty `proxy=` is unset, so it must not suppress the environment
    // fallback. `false` and `null` keep their meaning: proxying is off.
    const legacyProxy = pnpmConfig.proxy === '' ? undefined : pnpmConfig.proxy
    pnpmConfig.httpsProxy = legacyProxy ?? getProcessEnv('https_proxy')
  }
  if (!pnpmConfig.httpProxy) {
    pnpmConfig.httpProxy = pnpmConfig.httpsProxy ?? getProcessEnv('http_proxy') ?? getProcessEnv('proxy')
  }
  if (!pnpmConfig.noProxy) {
    // @ts-expect-error -- noproxy (lowercase) is a config-file key, not a Config field
    pnpmConfig.noProxy = pnpmConfig['noproxy'] ?? getProcessEnv('no_proxy')
  }
}

function applyNodeLinkerDefaults (pnpmConfig: PnpmConfigInProgress): void {
  if (pnpmConfig.nodeLinker === 'pnp') {
    pnpmConfig.enablePnp = true
  } else if (pnpmConfig.nodeLinker === 'hoisted' && pnpmConfig.preferSymlinkedExecutables == null) {
    pnpmConfig.preferSymlinkedExecutables = true
  }
}

function resolveDependencyTypes (pnpmConfig: PnpmConfigInProgress): void {
  if (pnpmConfig.only === 'prod' || pnpmConfig.only === 'production' || !pnpmConfig.only && pnpmConfig.production) {
    pnpmConfig.production = true
    pnpmConfig.dev = false
  } else if (pnpmConfig.only === 'dev' || pnpmConfig.only === 'development' || pnpmConfig.dev) {
    pnpmConfig.production = false
    pnpmConfig.dev = true
  } else {
    pnpmConfig.production = true
    pnpmConfig.dev = true
  }
}

/**
 * With a global virtual store, package directories live outside the
 * project, so Node's upward node_modules walk from their real paths never
 * reaches the project's hoisted node_modules or root node_modules. Expose
 * both through NODE_PATH for every child process pnpm spawns, and register
 * the ESM loader that restores NODE_PATH lookups for ESM imports.
 */
function extendNodePathForGlobalVirtualStore (pnpmConfig: PnpmConfigInProgress, env: Record<string, string | undefined>): void {
  if (
    !pnpmConfig.enableGlobalVirtualStore ||
    pnpmConfig.extendNodePath === false ||
    (pnpmConfig.nodeLinker != null && pnpmConfig.nodeLinker !== 'isolated')
  ) return
  const modulesDir = pathAbsolute(pnpmConfig.modulesDir ?? 'node_modules', pnpmConfig.rootProjectManifestDir)
  const nodePaths = [
    ...(pnpmConfig.extraEnv['NODE_PATH']?.split(path.delimiter) ?? []),
    path.join(modulesDir, '.pnpm', 'node_modules'),
    modulesDir,
  ]
  pnpmConfig.extraEnv['NODE_PATH'] = Array.from(new Set(nodePaths)).join(path.delimiter)
  pnpmConfig.extraEnv['NODE_OPTIONS'] = addEsmNodePathLoaderOption(env['NODE_OPTIONS'])
}
