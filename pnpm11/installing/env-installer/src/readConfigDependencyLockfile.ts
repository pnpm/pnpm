import path from 'node:path'

import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { createEnvLockfile, type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import type { ConfigDependencies } from '@pnpm/types'

/** Loads reconciliation state without opening the store or initializing resolvers. */
export async function readConfigDependencyLockfile (
  configDependencies: ConfigDependencies,
  rootDir: string
): Promise<EnvLockfile | undefined> {
  const env = await readEnvLockfile(rootDir)
  if (Object.keys(configDependencies).length || Object.keys(env?.importers['.'].configDependencies ?? {}).length) {
    return env ?? createEnvLockfile()
  }
  const installed = await readModulesDir(path.join(rootDir, 'node_modules/.pnpm-config'))
  return installed?.length ? env ?? createEnvLockfile() : undefined
}
