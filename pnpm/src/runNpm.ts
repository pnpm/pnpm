import { packageManager } from '@pnpm/cli-meta'
import { getConfig, types as allTypes } from '@pnpm/config'
import { runNpm as _runNpm } from '@pnpm/run-npm'
import pick from 'ramda/src/pick'

export async function runNpm (args: string[]) {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
    rcOptionsTypes: {
      ...pick([
        'npm-path',
      ], allTypes),
    },
  })
  return _runNpm(config.npmPath, args)
}
