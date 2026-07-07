import { packageManager } from '@pnpm/cli.meta'
import type { Config } from '@pnpm/config.reader'
import { updateCheckLogger } from '@pnpm/core-loggers'
import { createResolver } from '@pnpm/installing.client'
import { globalWarn } from '@pnpm/logger'

import { readPnpmState, updatePnpmState } from './pnpmState.js'

const UPDATE_CHECK_FREQUENCY = 24 * 60 * 60 * 1000 // 1 day

export async function checkForUpdates (config: Config): Promise<void> {
  // The configured stateDir (unlike the pnpmExecCommand trust records, which
  // use the default per-user dir): the update-check timestamp is a
  // performance hint, not a security signal, so a workspace-set stateDir
  // costs at most an extra registry query.
  const { state, readError } = await readPnpmState(config.stateDir)
  if (readError != null) {
    // Persistence is skipped for an unreadable state file, so the check will
    // repeat every run; surface why instead of degrading silently.
    globalWarn(readError.message)
  }

  if (
    state?.lastUpdateCheck &&
    (Date.now() - new Date(state.lastUpdateCheck).valueOf()) < UPDATE_CHECK_FREQUENCY
  ) return

  const { resolve } = createResolver({
    ...config,
    configByUri: config.configByUri,
    retry: {
      retries: 0,
    },
  })
  const resolution = await resolve({ alias: packageManager.name, bareSpecifier: 'latest' }, {
    lockfileDir: config.lockfileDir ?? config.dir,
    preferredVersions: {},
    projectDir: config.dir,
  })
  if (resolution?.manifest?.version) {
    updateCheckLogger.debug({
      currentVersion: packageManager.version,
      latestVersion: resolution?.manifest.version,
    })
  }
  await updatePnpmState(config.stateDir, () => ({
    lastUpdateCheck: new Date().toUTCString(),
  }))
}
