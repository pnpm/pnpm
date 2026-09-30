import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'

import { checkGlobalBinDir } from '../checkGlobalBinDir.js'
import type { CliOptions, ConfigBuildState, PnpmConfigInProgress } from './configBuildState.js'

export interface GlobalBinDirCheckOptions {
  globalDirShouldAllowWrite?: boolean
  skipGlobalBinDirCheck?: boolean
}

/** Adjusts the config for a command run with `--global`. */
export async function applyGlobalInstallSettings (
  { cliOptions, env, pnpmConfig }: ConfigBuildState,
  opts: GlobalBinDirCheckOptions
): Promise<void> {
  delete pnpmConfig.workspaceDir
  pnpmConfig.bin = pnpmConfig.globalBinDir ?? path.join(pnpmConfig.pnpmHomeDir, 'bin')
  if (pnpmConfig.bin && !opts.skipGlobalBinDirCheck) {
    fs.mkdirSync(pnpmConfig.bin, { recursive: true })
    await checkGlobalBinDir(pnpmConfig.bin, { env, shouldAllowWrite: opts.globalDirShouldAllowWrite })
  }
  pnpmConfig.save = true
  pnpmConfig.allowNew = true
  pnpmConfig.ignoreCurrentSpecifiers = true
  pnpmConfig.saveProd = true
  pnpmConfig.saveDev = false
  pnpmConfig.saveOptional = false
  if (hasCustomHoistPattern(pnpmConfig) && cliOptions['hoist-pattern']) {
    throw new PnpmError('CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL',
      'Configuration conflict. "hoist-pattern" may not be used with "global"')
  }
  disableWorkspaceSettingsForGlobal(pnpmConfig, cliOptions)
  if (cliOptions['virtual-store-dir']) {
    throw new PnpmError('CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL',
      'Configuration conflict. "virtual-store-dir" may not be used with "global"')
  }
  if (pnpmConfig.enableGlobalVirtualStore == null) {
    pnpmConfig.enableGlobalVirtualStore = true
  }
}

function hasCustomHoistPattern (pnpmConfig: PnpmConfigInProgress): boolean {
  return (pnpmConfig.hoistPattern != null) && (pnpmConfig.hoistPattern.length > 1 || pnpmConfig.hoistPattern[0] !== '*')
}

function disableWorkspaceSettingsForGlobal (pnpmConfig: PnpmConfigInProgress, cliOptions: CliOptions): void {
  if (pnpmConfig.linkWorkspacePackages) {
    if (cliOptions['link-workspace-packages']) {
      throw new PnpmError('CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL',
        'Configuration conflict. "link-workspace-packages" may not be used with "global"')
    }
    pnpmConfig.linkWorkspacePackages = false
  }
  if (pnpmConfig.sharedWorkspaceLockfile) {
    if (cliOptions['shared-workspace-lockfile']) {
      throw new PnpmError('CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL',
        'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
    }
    pnpmConfig.sharedWorkspaceLockfile = false
  }
  if (pnpmConfig.lockfileDir) {
    if (cliOptions['lockfile-dir']) {
      throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL',
        'Configuration conflict. "lockfile-dir" may not be used with "global"')
    }
    delete pnpmConfig.lockfileDir
  }
}
