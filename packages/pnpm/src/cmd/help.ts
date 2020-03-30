import packageManager from '@pnpm/cli-meta'
import renderHelp = require('render-help')

export default function (helpByCommandName: Record<string, () => string>) {
  return function (opts: {}, params: string[]) {
    let helpText!: string
    if (params.length === 0) {
      helpText = getHelpText()
    } else if (helpByCommandName[params[0]]) {
      helpText = helpByCommandName[params[0]]()
    } else {
      helpText = `No results for "${params[0]}"`
    }
    return `Version ${packageManager.version}\n${helpText}\n`
  }
}

function getHelpText () {
  return renderHelp({
    descriptionLists: [
      {
        title: 'Manage your dependencies',

        list: [
          {
            description: 'Install all dependencies for a project',
            name: 'install',
            shortAlias: 'i',
          },
          {
            description: 'Installs a package and any packages that it depends on. By default, any new package is installed as a prod dependency',
            name: 'add',
          },
          {
            description: 'Updates packages to their latest version based on the specified range',
            name: 'update',
            shortAlias: 'up',
          },
          {
            description: 'Removes packages from node_modules and from the project\'s package.json',
            name: 'remove',
            shortAlias: 'rm',
          },
          {
            description: 'Connect the local project to another one',
            name: 'link',
            shortAlias: 'ln',
          },
          {
            description: 'Unlinks a package. Like yarn unlink but pnpm re-installs the dependency after removing the external link',
            name: 'unlink',
          },
          {
            description: 'Generates a pnpm-lock.yaml from an npm package-lock.json (or npm-shrinkwrap.json) file',
            name: 'import',
          },
          {
            description: 'Runs a pnpm install followed immediately by a pnpm test',
            name: 'install-test',
            shortAlias: 'it',
          },
          {
            description: 'Rebuild a package',
            name: 'rebuild',
            shortAlias: 'rb',
          },
          {
            description: 'Removes extraneous packages',
            name: 'prune',
          },
        ],
      },
      {
        title: 'Review your dependencies',

        list: [
          {
            description: 'Print all the versions of packages that are installed, as well as their dependencies, in a tree-structure',
            name: 'list',
            shortAlias: 'ls',
          },
          {
            description: 'Check for outdated packages',
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
