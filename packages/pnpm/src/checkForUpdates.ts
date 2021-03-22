import { homedir } from 'os'
import path from 'path'
import packageManager from '@pnpm/cli-meta'
import { Config } from '@pnpm/config'
import { createResolver } from '@pnpm/client'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import storePath from '@pnpm/store-path'
import { updateCheckLogger } from '@pnpm/core-loggers'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'

interface State {
  lastUpdateCheck?: string
}

const UPDATE_CHECK_FREQUENCY = 24 * 60 * 60 * 1000 // 1 day

export default async function (config: Config) {
  const stateFile = path.join(homedir(), '.pnpm-state.json')
  let state: State | undefined
  try {
    state = await loadJsonFile(stateFile)
  } catch (err) {}

  if (
    state?.lastUpdateCheck &&
    (Date.now() - new Date(state.lastUpdateCheck).valueOf()) < UPDATE_CHECK_FREQUENCY
  ) return

  try {
    const storeDir = await storePath(config.dir, config.storeDir)
    const resolve = createResolver({
      ...config,
      authConfig: config.rawConfig,
      storeDir,
    })
    const resolution = await resolve({ alias: packageManager.name, pref: 'latest' }, {
      lockfileDir: config.lockfileDir ?? config.dir,
      preferredVersions: {},
      projectDir: config.dir,
      registry: pickRegistryForPackage(config.registries, packageManager.name, 'latest'),
    })
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
  } catch (err) {
    // ignore any issues
  }
}
