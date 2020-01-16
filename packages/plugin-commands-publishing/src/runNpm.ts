import { packageManager } from '@pnpm/cli-utils'
import getConfig, { types as allTypes } from '@pnpm/config'
import runNpm from '@pnpm/run-npm'
import R = require('ramda')

export default async function run (args: string[]) {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
    rcOptionsTypes: {
      ...R.pick([
        'npm-path',
      ], allTypes),
    },
  })
  return runNpm(config.npmPath, args)
}
