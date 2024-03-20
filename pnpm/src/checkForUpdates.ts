import path from 'node:path'

import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'

import type { Config } from '@pnpm/types'
import { createResolver } from '@pnpm/client'
import { packageManager } from '@pnpm/cli-meta'
import { updateCheckLogger } from '@pnpm/core-loggers'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'

interface State {
  lastUpdateCheck?: string
}

const UPDATE_CHECK_FREQUENCY = 24 * 60 * 60 * 1000 // 1 day

export async function checkForUpdates(config: Config): Promise<void> {
  const stateFile = path.join(config.stateDir, 'pnpm-state.json')

  let state: State | undefined

  try {
    state = await loadJsonFile(stateFile)
  } catch (err) {}

  if (
    state?.lastUpdateCheck &&

    Date.now() - new Date(state.lastUpdateCheck).valueOf() <
      UPDATE_CHECK_FREQUENCY
  ) {
    return
  }

  const resolve = createResolver({
    ...config,
    authConfig: config.rawConfig,
    retry: {
      retries: 0,
    },
  })

  const resolution = await resolve(
    { alias: packageManager.name, pref: 'latest' },
    {
      lockfileDir: config.lockfileDir ?? config.dir,
      preferredVersions: {},
      projectDir: config.dir,
      registry: pickRegistryForPackage(
        config.registries,
        packageManager.name,
        'latest'
      ),
    }
  )

  if (resolution?.manifest?.version) {
    updateCheckLogger.debug({
      currentVersion: packageManager.version,
      latestVersion: resolution?.manifest.version,
    })
  }

  await writeJsonFile(stateFile, {
    ...state,
    lastUpdateCheck: new Date().toUTCString(),
  })
}
