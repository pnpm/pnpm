import { homedir } from 'os'
import path from 'path'
import packageManager from '@pnpm/cli-meta'
import { Config } from '@pnpm/config'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
import { createManifestGetter } from '@pnpm/outdated/lib/createManifestGetter'
import storePath from '@pnpm/store-path'
import { updateCheckLogger } from '@pnpm/core-loggers'

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
    const manifestGetter = createManifestGetter({
      ...config,
      fullMetadata: false,
      lockfileDir: config.lockfileDir ?? config.dir,
      storeDir,
    })
    const latestManifest = await manifestGetter(packageManager.name, 'latest')
    if (latestManifest?.version) {
      updateCheckLogger.debug({
        currentVersion: packageManager.version,
        latestVersion: latestManifest.version,
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
