import type { SpawnSyncReturns } from 'node:child_process'

import pick from 'ramda/src/pick'

import { packageManager } from '@pnpm/cli-meta'
import { runNpm as _runNpm } from '@pnpm/run-npm'
import { getConfig, types as allTypes } from '@pnpm/config'

export async function runNpm(args: string[]): Promise<SpawnSyncReturns<Buffer>> {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
    rcOptionsTypes: {
      ...pick(['npm-path'], allTypes),
    },
  })

  return _runNpm(config.npmPath, args)
}
