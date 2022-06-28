import packageManager from '@pnpm/cli-meta'
import getConfig, { types as allTypes } from '@pnpm/config'
import runNpm from '@pnpm/run-npm'
import pick from 'ramda/src/pick.js'

export default async function run (args: string[]) {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
    rcOptionsTypes: {
      ...pick([
        'npm-path',
      ], allTypes),
    },
  })
  return runNpm(config.npmPath, args)
}
