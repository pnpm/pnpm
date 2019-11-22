import { packageManager } from '@pnpm/cli-utils'
import renderHelp = require('render-help')

export default function (helpByCommandName: Record<string, () => string>) {
  return function (input: string[]) {
    const helpText = input.length === 0 ? getHelpText() : helpByCommandName[input[0]]()
    console.log(`Version ${packageManager.version}\n${helpText}`)
  }
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
