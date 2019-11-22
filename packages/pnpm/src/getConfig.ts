import { packageManager } from '@pnpm/cli-utils'
import getConfig from '@pnpm/config'

export default async function (
  cliArgs: object,
  opts: {
    excludeReporter: boolean,
    command: string[],
  },
) {
  const { config, warnings } = await getConfig({
    cliArgs,
    command: opts.command,
    packageManager,
  })
  config.cliArgs = cliArgs

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because supi expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.log(warnings.join('\n'))
  }

  return config
}
