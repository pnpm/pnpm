import os from 'node:os'

import { applyRuntimeOnFailOverride } from '@pnpm/pkg-manifest.utils'

import { transformPathKeys } from '../transformPath.js'
import type { ConfigBuildState, PnpmConfigInProgress } from './configBuildState.js'
import { getNodeVersionFromEnginesRuntime } from './packageManager.js'
import { applyRemoteSideEffectsCacheEnv } from './sideEffectsCache.js'

export function finalizeConfig ({ cliOptions, env, pnpmConfig }: ConfigBuildState): void {
  // The yes option is only meant to be a CLI option. Remove it from the
  // returned pnpm config.
  delete (pnpmConfig as { yes?: boolean }).yes
  if (cliOptions.yes) {
    pnpmConfig.autoConfirmAllPrompts = true
  }

  transformPathKeys(pnpmConfig, os.homedir())

  applyPmOnFail(pnpmConfig)
  applyRuntimeOnFail(pnpmConfig)

  if (pnpmConfig.nodeVersion == null && pnpmConfig.enginePinManifest != null) {
    pnpmConfig.nodeVersion = getNodeVersionFromEnginesRuntime(pnpmConfig.enginePinManifest)
    pnpmConfig.nodeVersionFromEnginesRuntime = pnpmConfig.nodeVersion != null
  }

  applyRemoteSideEffectsCacheEnv(pnpmConfig, env)
}

/**
 * The `pmOnFail` config setting overrides whatever onFail the
 * wantedPackageManager carried, so users (and internal callers) can force
 * a specific behavior without editing the manifest. Otherwise, both the
 * legacy `packageManager` field and singular `devEngines.packageManager`
 * fall through to `download` (the documented default for `pmOnFail`); the
 * array form of `devEngines.packageManager` already has its own per-element
 * defaults applied during parsing.
 */
function applyPmOnFail (pnpmConfig: PnpmConfigInProgress): void {
  if (!pnpmConfig.wantedPackageManager) return
  if (pnpmConfig.pmOnFail) {
    pnpmConfig.wantedPackageManager.onFail = pnpmConfig.pmOnFail
  } else if (pnpmConfig.wantedPackageManager.onFail == null) {
    pnpmConfig.wantedPackageManager.onFail = 'download'
  }
}

function applyRuntimeOnFail (pnpmConfig: PnpmConfigInProgress): void {
  if (!pnpmConfig.runtimeOnFail) return
  if (pnpmConfig.rootProjectManifest) {
    applyRuntimeOnFailOverride(pnpmConfig.rootProjectManifest, pnpmConfig.runtimeOnFail)
  }
  if (pnpmConfig.enginePinManifest && pnpmConfig.enginePinManifest !== pnpmConfig.rootProjectManifest) {
    applyRuntimeOnFailOverride(pnpmConfig.enginePinManifest, pnpmConfig.runtimeOnFail)
  }
}
