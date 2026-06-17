import { type EnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'

import { verifyEnvLockfile } from './verifyEnvLockfile.js'

// The single path for persisting an env lockfile: it is verified (the offline
// structural gate) before it touches disk, so no code path can write an env
// lockfile carrying an invalid config-dependency name or version.
export async function writeVerifiedEnvLockfile (rootDir: string, envLockfile: EnvLockfile): Promise<void> {
  verifyEnvLockfile(envLockfile)
  await writeEnvLockfile(rootDir, envLockfile)
}
