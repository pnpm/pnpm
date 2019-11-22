import { oneLine } from 'common-tags'
import renderHelp = require('render-help')
import pnpmPkgJson from '../pnpmPkgJson'

export default function (helpByCommandName: Record<string, () => string>) {
  return function (input: string[]) {
    const helpText = input.length === 0 ? getHelpText() : helpByCommandName[input[0]]()
    console.log(`Version ${pnpmPkgJson.version}\n${helpText}`)
  }
}

export const OPTIONS = {
  globalDir: {
    description: 'Specify a custom directory to store global packages',
    name: '--global-dir',
  },
  ignoreScripts: {
    description: "Don't run lifecycle scripts",
    name: '--ignore-scripts',
  },
  offline: {
    description: 'Trigger an error if any required dependencies are not available in local store',
    name: '--offline',
  },
  preferOffline: {
    description: 'Skip staleness checks for cached data, but request missing data from the server',
    name: '--prefer-offline',
  },
  storeDir: {
    description: 'The directory in which all the packages are saved on the disk',
    name: '--store-dir <dir>',
  },
  virtualStoreDir: {
    description: oneLine`
      The directory with links to the store (default is node_modules/.pnpm).
      All direct and indirect dependencies of the project are linked into this directory`,
    name: '--virtual-store-dir <dir>',
  }
}

export const UNIVERSAL_OPTIONS = [
  {
    description: 'Controls colors in the output. By default, output is always colored when it goes directly to a terminal',
    name: '--[no-]color',
  },
  {
    description: 'Output usage information',
    name: '--help',
    shortAlias: '-h',
  },
  {
    description: `Change to directory <dir> (default: ${process.cwd()})`,
    name: '--dir <dir>',
    shortAlias: '-C',
  },
]
export const FILTERING = {
  list: [
    {
      description: 'Restricts the scope to package names matching the given pattern. E.g.: foo, @bar/*',
      name: '--filter <pattern>',
    },
    {
      description: 'Includes all direct and indirect dependencies of the matched packages. E.g.: foo...',
      name: '--filter <pattern>...',
    },
    {
      description: 'Includes all direct and indirect dependents of the matched packages. E.g.: ...foo, ...@bar/*',
      name: '--filter ...<pattern>',
    },
    {
      description: 'Includes all packages that are inside a given subdirectory. E.g.: ./components',
      name: '--filter ./<dir>',
    },
    {
      description: 'Includes all packages that are under the current working directory',
      name: '--filter .',
    },
  ],
  title: 'Filtering options (run the command only on packages that satisfy at least one of the selectors)',
}

function getHelpText () {
  return renderHelp({
    descriptionLists: [
      {
        title: 'Manage your dependencies',

        list: [
          {
            name: 'install',
            shortAlias: 'i',
          },
          {
            name: 'add',
          },
          {
            name: 'update',
            shortAlias: 'up',
          },
          {
            name: 'remove',
            shortAlias: 'rm',
          },
          {
            name: 'link',
            shortAlias: 'ln',
          },
          {
            name: 'unlink',
          },
          {
            name: 'import',
          },
          {
            name: 'install-test',
            shortAlias: 'it',
          },
          {
            name: 'rebuild',
            shortAlias: 'rb',
          },
          {
            name: 'prune',
          },
        ],
      },
      {
        title: 'Review your dependencies',

        list: [
          {
            name: 'list',
            shortAlias: 'ls',
          },
          {
            name: 'outdated',
          },
        ],
      },
      {
        title: 'Run your scripts',

        list: [
          {
            name: 'run',
          },
          {
            name: 'test',
            shortAlias: 't',
          },
          {
            name: 'start',
          },
          {
            name: 'restart',
          },
          {
            name: 'stop',
          },
        ],
      },
      {
        title: 'Other',

        list: [
          {
            name: 'pack',
          },
          {
            name: 'publish',
          },
          {
            name: 'root',
          },
          {
            name: 'audit',
          },
        ],
      },
      {
        title: 'Manage your monorepo',

        list: [
          {
            name: 'recursive exec',
          },
          {
            name: 'recursive install',
          },
          {
            name: 'recursive add',
          },
          {
            name: 'recursive list',
          },
          {
            name: 'recursive why',
          },
          {
            name: 'recursive outdated',
          },
          {
            name: 'recursive rebuild',
          },
          {
            name: 'recursive run',
          },
          {
            name: 'recursive test',
          },
          {
            name: 'recursive remove',
          },
          {
            name: 'recursive unlink',
          },
          {
            name: 'recursive update',
          },
        ],
      },
      {
        title: 'Use a store server',

        list: [
          {
            name: 'server start',
          },
          {
            name: 'server status',
          },
          {
            name: 'server stop',
          },
        ],
      },
      {
        title: 'Manage your store',

        list: [
          {
            name: 'store add',
          },
          {
            name: 'store prune',
          },
          {
            name: 'store status',
          },
        ],
      },
    ],
    usages: ['pnpm [command] [flags]', 'pnpm [ -h | --help | -v | --version ]'],
  })
}
