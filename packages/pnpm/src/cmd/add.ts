import { oneLine } from 'common-tags'
import renderHelp = require('render-help')
import { PnpmOptions } from '../types'
import { docsUrl, FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from './help'
import { handler as install } from './install'

export const commandNames = ['add']

export function help () {
  return renderHelp({
    description: 'Installs a package and any packages that it depends on.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Save package to your \`dependencies\`. The default behavior',
            name: '--save-prod',
            shortAlias: '-P',
          },
          {
            description: 'Save package to your \`devDependencies\`',
            name: '--save-dev',
            shortAlias: '-D',
          },
          {
            description: 'Save package to your \`optionalDependencies\`',
            name: '--save-optional',
            shortAlias: '-O',
          },
          {
            description: 'Save package to your \`peerDependencies\` and \`devDependencies\`',
            name: '--save-peer',
          },
          {
            description: 'Install exact version',
            name: '--[no-]save-exact',
            shortAlias: '-E',
          },
          {
            description: 'Save packages from the workspace with a "workspace:" protocol. True by default',
            name: '--[no-]save-workspace-protocol',
          },
          {
            description: 'Install as a global package',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: oneLine`Run installation recursively in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('add'),
    usages: [
      'pnpm add <name>',
      'pnpm add <name>@<tag>',
      'pnpm add <name>@<version>',
      'pnpm add <name>@<version range>',
      'pnpm add <git host>:<git user>/<repo name>',
      'pnpm add <git repo url>',
      'pnpm add <tarball file>',
      'pnpm add <tarball url>',
      'pnpm add <dir>',
    ],
  })
}

export async function handler (
  input: string[],
  opts: PnpmOptions & {
    allowNew?: boolean,
    update?: boolean,
    useBetaCli?: boolean,
  },
  invocation?: string,
) {
  return install(input, opts, invocation)
}
