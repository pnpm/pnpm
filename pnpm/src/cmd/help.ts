import { packageManager } from '@pnpm/cli-meta'
import renderHelp from 'render-help'

export function createHelp (helpByCommandName: Record<string, () => string>) {
  return function (opts: unknown, params: string[]) {
    let helpText!: string
    if (params.length === 0) {
      helpText = getHelpText()
    } else if (helpByCommandName[params[0]]) {
      helpText = helpByCommandName[params[0]]()
    } else {
      helpText = `No results for "${params[0]}"`
    }
    return `Version ${packageManager.version}\
${process['pkg'] != null ? ` (compiled to binary; bundled Node.js ${process.version})` : ''}\
\n${helpText}\n`
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
            description: 'Checks for known security issues with the installed packages',
            name: 'audit',
          },
          {
            description: 'Print all the versions of packages that are installed, as well as their dependencies, in a tree-structure',
            name: 'list',
            shortAlias: 'ls',
          },
          {
            description: 'Check for outdated packages',
            name: 'outdated',
          },
          {
            description: 'Check licenses in consumed packages',
            name: 'licenses',
          },
        ],
      },
      {
        title: 'Run your scripts',

        list: [
          {
            description: 'Executes a shell command in scope of a project',
            name: 'exec',
          },
          {
            description: 'Runs a defined package script',
            name: 'run',
          },
          {
            description: 'Runs a package\'s "test" script, if one was provided',
            name: 'test',
            shortAlias: 't',
          },
          {
            description: 'Runs an arbitrary command specified in the package\'s "start" property of its "scripts" object',
            name: 'start',
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
            description: 'Publishes a package to the registry',
            name: 'publish',
          },
          {
            name: 'root',
          },
        ],
      },
      {
        title: 'Manage your store',

        list: [
          {
            description: 'Adds new packages to the pnpm store directly. Does not modify any projects or files outside the store',
            name: 'store add',
          },
          {
            description: 'Prints the path to the active store directory',
            name: 'store path',
          },
          {
            description: 'Removes unreferenced (extraneous, orphan) packages from the store',
            name: 'store prune',
          },
          {
            description: 'Checks for modified packages in the store',
            name: 'store status',
          },
        ],
      },
      {
        title: 'Options',

        list: [
          {
            description: 'Run the command for each project in the workspace.',
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
    ],
    usages: ['pnpm [command] [flags]', 'pnpm [ -h | --help | -v | --version ]'],
  })
}
