import { packageManager, detectIfCurrentPkgIsExecutable } from '@pnpm/cli-meta'
import renderHelp from 'render-help'
import { type CommandDefinition } from './index.js'

export function createHelp (helpByCommandName: Record<string, () => string>): CommandDefinition {
  return {
    commandNames: ['help'],
    shorthands: {
      a: '--all',
    },
    cliOptionsTypes: () => ({
      all: Boolean,
    }),
    rcOptionsTypes: () => ({}),
    help: () => getHelpText({ all: false }),
    handler: function (opts: { all?: boolean }, params: string[]) {
      let helpText!: string
      if (params.length === 0) {
        helpText = getHelpText({ all: opts.all ?? false })
      } else if (helpByCommandName[params[0]]) {
        helpText = helpByCommandName[params[0]]()
      } else {
        helpText = `No results for "${params[0]}"`
      }
      return `Version ${packageManager.version}\
  ${detectIfCurrentPkgIsExecutable() != null ? ` (compiled to binary; bundled Node.js ${process.version})` : ''}\
  \n${helpText}\n`
    },
  }
}

interface DescriptionItem {
  shortAlias?: string
  name: string
  description?: string
  advanced?: boolean
}

type DescriptionLists = Array<{
  title: string
  advanced?: boolean
  list: DescriptionItem[]
}>

function getHelpText ({ all }: { all: boolean }): string {
  const descriptionLists: DescriptionLists = [
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
          advanced: true,
        },
        {
          description: 'Rebuild a package',
          name: 'rebuild',
          shortAlias: 'rb',
          advanced: true,
        },
        {
          description: 'Removes extraneous packages',
          name: 'prune',
          advanced: true,
        },
        {
          description: 'Fetch packages from a lockfile into virtual store, package manifest is ignored',
          name: 'fetch',
          advanced: true,
        },
        {
          description: 'Perform an install removing older dependencies in the lockfile if a newer version can be used',
          name: 'dedupe',
          advanced: true,
        },
      ],
    },
    {
      title: 'Patch your dependencies',

      list: [
        {
          description: 'Prepare a package for patching',
          name: 'patch',
        },
        {
          description: 'Generate a patch out of a directory',
          name: 'patch-commit',
          advanced: true,
        },
        {
          description: 'Remove existing patch files',
          name: 'patch-remove',
          advanced: true,
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
        {
          description: 'Shows all packages that depend on the specified package',
          name: 'why',
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
        {
          description: 'Fetches a package from the registry without installing it as a dependency, hot loads it, and runs whatever default command binary it exposes',
          name: 'dlx',
        },
        {
          description: 'Create a project from a "create-*" or "@foo/create-*" starter kit',
          name: 'create',
        },
      ],
    },
    {
      title: 'Other',

      list: [
        {
          description: 'Create a tarball from a package',
          name: 'pack',
        },
        {
          description: 'Publishes a package to the registry',
          name: 'publish',
        },
        {
          description: 'Updates pnpm to the latest version.',
          name: 'self-update',
        },
        {
          description: 'Create a package.json file',
          name: 'init',
        },
        {
          description: 'Manage the pnpm configuration files',
          name: 'config',
          shortAlias: 'c',
        },
        {
          description: 'Prints the effective modules directory',
          name: 'root',
          advanced: true,
        },
        {
          description: 'Prints the directory into which the executables of dependencies are linked',
          name: 'bin',
          advanced: true,
        },
        {
          description: 'Deploy a package from a workspace',
          name: 'deploy',
          advanced: true,
        },
        {
          description: 'Checks for known common issues with pnpm configuration',
          name: 'doctor',
          advanced: true,
        },
      ],
    },
    {
      title: 'Manage your environments',

      list: [
        {
          description: 'Manage Node.js versions',
          name: 'env ',
        },
      ],
    },
    {
      title: 'Inspect your store',
      advanced: true,

      list: [
        {
          description: 'Prints the index file of a specific package from the store',
          name: 'cat-index',
        },
        {
          description: 'Prints the contents of a file based on the hash value stored in the index file',
          name: 'cat-file',
        },
        {
          description: 'Experimental! Lists the packages that include the file with the specified hash',
          name: 'find-hash',
        },
      ],
    },
    {
      title: 'Manage your store',
      advanced: true,

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
      title: 'Manage your cache',
      advanced: true,

      list: [
        {
          description: 'Experimental! Lists the available packages metadata cache. Supports filtering by glob',
          name: 'cache list',
        },
        {
          description: 'Experimental! Lists all registries that have their metadata cache locally',
          name: 'cache list-registries',
        },
        {
          description: 'Experimental! Views information from the specified package\'s cache',
          name: 'cache view',
        },
        {
          description: 'Experimental! Deletes metadata cache for the specified package(s). Supports patterns',
          name: 'cache delete',
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
  ]
  const filteredDescriptionLists = []
  for (const descriptionList of descriptionLists) {
    if (!descriptionList.advanced || all) {
      filteredDescriptionLists.push({
        title: descriptionList.title,
        list: descriptionList.list.filter((item) => !item.advanced || all),
      })
    }
  }
  return renderHelp({
    description: all ? '' : 'These are common pnpm commands used in various situations, use \'pnpm --help -a\' to list all commands',
    descriptionLists: filteredDescriptionLists,
    usages: ['pnpm [command] [flags]', 'pnpm [ -h | --help | -v | --version ]'],
  })
}
