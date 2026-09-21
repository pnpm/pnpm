import { docsUrl } from '@pnpm/cli.utils'
import { binDirOf, type Config, type ConfigContext, projectModulesDir, types as allTypes } from '@pnpm/config.reader'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'global',
  ], allTypes)
}

export const commandNames = ['bin']

export function help (): string {
  return renderHelp({
    description: 'Print the directory where pnpm will install executables.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global executables directory',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('bin'),
    usages: ['pnpm bin [-g]'],
  })
}

export async function handler (
  opts: Pick<Config, 'packageConfigs' | 'lockfileDir' | 'modulesDir'> & Pick<ConfigContext, 'cliOptions'> & {
    bin: string
    dir: string
  }
): Promise<string> {
  // `--global` already resolved the global directory, and only a workspace
  // whose projects keep their own lockfiles can have given this one a modules
  // directory of its own. An ordinary `pnpm bin` therefore reads no manifest,
  // and a project without one still answers.
  if (opts.cliOptions['global'] || opts.lockfileDir != null || opts.packageConfigs == null) {
    return opts.bin
  }
  const manifest = await safeReadProjectManifestOnly(opts.dir)
  return binDirOf(opts.dir, projectModulesDir(opts, manifest?.name))
}
