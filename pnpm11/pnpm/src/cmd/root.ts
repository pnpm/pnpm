import { docsUrl } from '@pnpm/cli.utils'
import { type Config, createProjectModulesDirResolver, modulesDirOf, types as allTypes } from '@pnpm/config.reader'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'global',
  ], allTypes)
}

export const commandNames = ['root']

export function help (): string {
  return renderHelp({
    description: 'Print the effective `node_modules` directory.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global packages directory',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('root'),
    usages: ['pnpm root [-g]'],
  })
}

export async function handler (
  opts: Pick<Config, 'packageConfigs' | 'lockfileDir' | 'modulesDir'> & {
    dir: string
    global?: boolean
    globalPkgDir?: string
  }
): Promise<string> {
  if (opts.global) {
    return `${opts.globalPkgDir}\n`
  }
  // Only a workspace whose projects keep their own lockfiles can have given
  // this project a modules directory of its own, so an ordinary `pnpm root`
  // reads no manifest, and a project without one still answers.
  let modulesDir = opts.modulesDir
  if (opts.lockfileDir == null && opts.packageConfigs != null) {
    const manifest = await safeReadProjectManifestOnly(opts.dir)
    modulesDir = createProjectModulesDirResolver(opts)(manifest?.name)
  }
  return `${modulesDirOf(opts.dir, modulesDir)}\n`
}
