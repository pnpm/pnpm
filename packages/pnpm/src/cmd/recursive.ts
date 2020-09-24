import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import renderHelp = require('render-help')

export const rcOptionsTypes = () => ({})
export const cliOptionsTypes = () => ({})

export const commandNames = ['recursive', 'multi', 'm']

export function help () {
  return renderHelp({
    description: 'Concurrently performs some actions in all subdirectories with a `package.json` (excluding node_modules). \
A `pnpm-workspace.yaml` file may be used to control what directories are searched for packages.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            name: 'install',
          },
          {
            name: 'add',
          },
          {
            name: 'update',
          },
          {
            description: 'Uninstall a dependency from each package',
            name: 'remove <pkg>...',
          },
          {
            description: 'Removes links to local packages and reinstalls them from the registry.',
            name: 'unlink',
          },
          {
            description: 'List dependencies in each package.',
            name: 'list [<pkg>...]',
          },
          {
            description: 'List packages that depend on <pkg>.',
            name: 'why <pkg>...',
          },
          {
            description: 'Check for outdated dependencies in every package.',
            name: 'outdated [<pkg>...]',
          },
          {
            description: 'This runs an arbitrary command from each package\'s "scripts" object. \
If a package doesn\'t have the command, it is skipped. \
If none of the packages have the command, the command fails.',
            name: 'run <command> [-- <args>...]',
          },
          {
            description: 'This runs each package\'s "test" script, if one was provided.',
            name: 'test [-- <args>...]',
          },
          {
            description: 'This command runs the "npm build" command on each package. \
This is useful when you install a new version of node, \
and must recompile all your C++ addons with the new binary.',
            name: 'rebuild [[<@scope>/<name>]...]',
          },
          {
            description: 'Run a command in each package.',
            name: 'exec -- <command> [args...]',
          },
          {
            description: 'Publishes packages to the npm registry. Only publishes a package if its version is not taken in the registry.',
            name: 'publish [--tag <tag>] [--access <public|restricted>]',
          },
        ],
      },
      {
        title: 'Options',

        list: [
          {
            description: 'Continues executing other tasks even if a task threw an error.',
            name: '--no-bail',
          },
          {
            description: 'Set the maximum number of concurrency. Default is 4. For unlimited concurrency use Infinity.',
            name: '--workspace-concurrency <number>',
          },
          {
            description: 'Locally available packages are linked to node_modules instead of being downloaded from the registry. \
Convenient to use in a multi-package repository.',
            name: '--link-workspace-packages',
          },
          {
            description: 'Sort packages topologically (dependencies before dependents). Pass --no-sort to disable.',
            name: '--sort',
          },
          {
            description: `Creates a single ${WANTED_LOCKFILE} file in the root of the workspace. \
A shared lockfile also means that all dependencies of all projects will be in a single node_modules.`,
            name: '--shared-workspace-lockfile',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('recursive'),
    usages: [
      'pnpm recursive [command] [flags] [--filter <package selector>]',
      'pnpm multi [command] [flags] [--filter <package selector>]',
      'pnpm m [command] [flags] [--filter <package selector>]',
    ],
  })
}

export function handler () {
  console.log(help())
  process.exit(1)
}
