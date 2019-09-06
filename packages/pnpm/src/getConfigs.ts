import getConfigs from '@pnpm/config'
import packageManager from './pnpmPkgJson'

export default async function (
  cliArgs: object,
  opts: {
    excludeReporter: boolean,
    command: string[],
  },
) {
  const { configs, warnings } = await getConfigs({
    cliArgs,
    command: opts.command,
    packageManager,
  })
  configs.cliArgs = cliArgs

  if (opts.excludeReporter) {
    delete configs.reporter // This is a silly workaround because supi expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.log(warnings.join('\n'))
  }

  return configs
}
