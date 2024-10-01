import { type SpawnSyncReturns } from 'child_process'
import { packageManager } from '@pnpm/cli-meta'
import { getConfig, types as allTypes } from '@pnpm/config'
import { runNpm as _runNpm } from '@pnpm/run-npm'
import pick from 'ramda/src/pick'

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
  if (args[0] === 'view' && args[2] === 'versions') {
    console.log('registry: ' + config.registry)
  }
  return _runNpm(config.npmPath, args)
}
