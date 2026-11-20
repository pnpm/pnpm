import path from 'node:path'

import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { createEnvLockfile, type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import type { ConfigDependencies } from '@pnpm/types'

export type ConfigDepsInstallMode = 'bootstrap' | 'reconcile'

/** Loads reconciliation state without opening the store or initializing resolvers. */
export async function readConfigDependencyLockfile (
  configDependencies: ConfigDependencies,
  rootDir: string,
  mode: ConfigDepsInstallMode = 'reconcile'
): Promise<EnvLockfile | undefined> {
  const hasDeclarations = Object.keys(configDependencies).length > 0
  if (mode === 'bootstrap' && !hasDeclarations) return undefined
  const env = await readEnvLockfile(rootDir)
  if (hasDeclarations || Object.keys(env?.importers['.'].configDependencies ?? {}).length) {
    return env ?? createEnvLockfile()
  }
  const installed = await readModulesDir(path.join(rootDir, 'node_modules/.pnpm-config'))
  return installed?.length ? env ?? createEnvLockfile() : undefined
}
