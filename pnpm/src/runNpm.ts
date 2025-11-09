import path from 'path'
import { type SpawnSyncReturns } from 'child_process'
import { packageManager } from '@pnpm/cli-meta'
import { getConfig, types as allTypes } from '@pnpm/config'
import { runNpm as _runNpm } from '@pnpm/run-npm'
import { pick } from 'ramda'

export async function runNpm (args: string[]): Promise<SpawnSyncReturns<Buffer>> {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
    rcOptionsTypes: {
      ...pick([
        'npm-path',
      ], allTypes),
    },
  })
  return _runNpm(config.npmPath, args, {
    // This code is only used in `passThruToNpm`, so it is safe to specify `userConfigPath` here.
    userConfigPath: path.join(config.configDir, 'rc'),
  })
}
