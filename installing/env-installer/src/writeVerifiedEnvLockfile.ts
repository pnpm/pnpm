import { type EnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'

import { verifyEnvLockfile } from './verifyEnvLockfile.js'

// Persist an env lockfile only after verifying it, so no code path can write
// one carrying an invalid config-dependency name or version.
export async function writeVerifiedEnvLockfile (rootDir: string, envLockfile: EnvLockfile): Promise<void> {
  verifyEnvLockfile(envLockfile)
  await writeEnvLockfile(rootDir, envLockfile)
}
