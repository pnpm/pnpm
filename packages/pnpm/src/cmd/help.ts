import { WANTED_LOCKFILE } from '@pnpm/constants'
import { oneLine, stripIndent } from 'common-tags'
import { table } from 'table'
import getCommandFullName from '../getCommandFullName'
import pnpmPkgJson from '../pnpmPkgJson'

export default function (input: string[]) {
  const cmdName = getCommandFullName(input[0])
  console.log(`Version ${pnpmPkgJson.version}\n${getHelpText(cmdName)}`)
}

const consoleWidth = process.stdout.columns || 80
const OPTIONS = {
  help: {
    description: 'Output usage information',
    name: '--help',
    shortAlias: '-h',
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
}
const FILTERING = {
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
      name: '--filter ./<directory>',
    },
    {
      description: 'Includes all packages that are under the current working directory',
      name: '--filter .',
    },
  ],
  title: 'Filtering options (run the command only on packages that satisfy at least one of the selectors)',
}

type DescriptionItem = { shortAlias?: string, name: string, description: string }

function renderHelp (
  config: {
    descriptionLists?: Array<{ title: string, list: DescriptionItem[] }>,
    description: string,
    usages: string[],
    url?: string,
  }
) {
  let output = ''

  if (config.usages.length > 0) {
    const [firstUsage, ...restUsages] = config.usages
    output += `Usage: ${firstUsage}\n`
    for (let usage of restUsages) {
      output += `       ${usage}\n`
    }
    output += '\n'
  }
  if (config.description) output += `${config.description}\n\n`
  if (config.descriptionLists) {
    for (let { title, list } of config.descriptionLists) {
      output += `${title}:\n` + renderDescriptionList(list)
    }
  }
  if (config.url) {
    output += `Visit ${config.url} for documentation about this command.`
  }
  return output
}

const NO_BORDERS = {
  topBody: '',
  topJoin: '',
  topLeft: '',
  topRight: '',

  bottomBody: '',
  bottomJoin: '',
  bottomLeft: '',
  bottomRight: '',

  bodyJoin: '',
  bodyLeft: '',
  bodyRight: '',

  joinBody: '',
  joinLeft: '',
  joinRight: '',
}
const TABLE_OPTIONS = {
  border: NO_BORDERS,
  singleLine: true,
}

const FIRST_COLUMN = { paddingLeft: 2 }
const SHORT_OPTION_COLUMN = { alignment: 'right' as const }
const LONG_OPTION_COLUMN = { paddingLeft: 0 }
const DESCRIPTION_COLUMN = {
  paddingRight: 0,
  wrapWord: true,
}

function renderDescriptionList (descriptionItems: DescriptionItem[]) {
  const data = descriptionItems.map(({ shortAlias, name, description }) => [shortAlias && `${shortAlias},` || '', name, description])
  const firstColumnMaxWidth = getColumnMaxWidth(data, 0)
  const descriptionColumnWidth = consoleWidth - firstColumnMaxWidth - getColumnMaxWidth(data, 1) - 2 - 2 - 1
  if (firstColumnMaxWidth === 0) {
    return table(data.map(([, ...row]) => row), {
      ...TABLE_OPTIONS,
      columns: [
        {
          ...LONG_OPTION_COLUMN,
          ...FIRST_COLUMN,
        },
        {
          width: descriptionColumnWidth,
          ...DESCRIPTION_COLUMN,
        },
      ],
    })
  }
  return table(data, {
    ...TABLE_OPTIONS,
    columns: [
      {
        ...SHORT_OPTION_COLUMN,
        ...FIRST_COLUMN,
      },
      LONG_OPTION_COLUMN,
      {
        width: descriptionColumnWidth,
        ...DESCRIPTION_COLUMN,
      },
    ],
  })
}

function getColumnMaxWidth (data: string[][], columnNumber: number) {
  return data.reduce((maxWidth, row) => Math.max(maxWidth, row[columnNumber].length), 0)
}

function getHelpText (command: string) {
  switch (getCommandFullName(command)) {
    case 'install':
      return renderHelp({
        description: 'Aliases: i\n\n' + oneLine`Installs all dependencies of the project in the current working directory.
          When executed inside a workspace, installs all dependencies of all workspace packages.`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Run installation recursively in every package found in subdirectories.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
              OPTIONS.ignoreScripts,
              OPTIONS.offline,
              OPTIONS.preferOffline,
              {
                description: "Packages in \`devDependencies\` won't be installed",
                name: '--production, --only prod[uction]',
              },
              {
                description: 'Only \`devDependencies\` are installed regardless of the \`NODE_ENV\`',
                name: '--only dev[elopment]',
              },
              {
                description: `Don't read or generate a \`${WANTED_LOCKFILE}\` file`,
                name: '--no-lockfile',
              },
              {
                description: `Dependencies are not downloaded. Only \`${WANTED_LOCKFILE}\` is updated`,
                name: '--lockfile-only',
              },
              {
                description: "Don't generate a lockfile and fail if an update is needed",
                name: '--frozen-lockfile',
              },
              {
                description: `If the available \`${WANTED_LOCKFILE}\` satisfies the \`package.json\` then perform a headless installation`,
                name: '--prefer-frozen-lockfile',
              },
              {
                description: `The directory in which the ${WANTED_LOCKFILE} of the package will be created. Several projects may share a single lockfile`,
                name: '--lockfile-directory <path>',
              },
              {
                description: 'Dependencies inside node_modules have access only to their listed dependencies',
                name: '--no-hoist',
              },
              {
                description: 'The subdeps will be hoisted into the root node_modules. Your code will have access to them',
                name: '--shamefully-hoist',
              },
              {
                description: 'Hoist all dependencies matching the pattern to the root of node_modules. Supplying it a * will hoist all dependencies (this is similar to what npm does)',
                name: '--hoist-pattern <pattern>',
              },
              {
                description: 'The location where all the packages are saved on the disk',
                name: '--store <path>',
              },
              {
                description: 'Maximum number of concurrent network requests',
                name: '--network-concurrency <number>',
              },
              {
                description: 'Controls the number of child processes run parallelly to build node modules',
                name: '--child-concurrency <number>',
              },
              {
                description: 'Disable pnpm hooks defined in pnpmfile.js',
                name: '--ignore-pnpmfile',
              },
              {
                description: 'Symlinks leaf dependencies directly from the global store',
                name: '--independent-leaves',
              },
              {
                description: "If false, doesn't check whether packages in the store were mutated",
                name: '--[no-]verify-store-integrity',
              },
              {
                description: '',
                name: '--[no-]lock',
              },
              {
                description: 'Fail on missing or invalid peer dependencies',
                name: '--strict-peer-dependencies',
              },
              {
                description: 'Starts a store server in the background. The store server will keep running after installation is done. To stop the store server, run \`pnpm server stop\`',
                name: '--use-store-server',
              },
              {
                description: 'Only allows installation with a store server. If no store server is running, installation will fail',
                name: '--use-running-store-server',
              },
              {
                description: 'Try to hardlink packages from the store. If it fails, fallback to copy',
                name: '--package-import-method auto',
              },
              {
                description: 'Hardlink packages from the store',
                name: '--package-import-method hardlink',
              },
              {
                description: 'Copy packages from the store',
                name: '--package-import-method copy',
              },
              {
                description: 'Reflink (aka copy-on-write) packages from the store',
                name: '--package-import-method reflink',
              },
              {
                description: 'The default resolution strategy. Speed is preferred over deduplication',
                name: '--resolution-strategy fast',
              },
              {
                description: 'Already installed dependencies are preferred even if newer versions satisfy a range',
                name: '--resolution-strategy fewer-dependencies',
              },
              OPTIONS.help,
            ],
          },
          {
            title: 'Output',

            list: [
              {
                description: 'No output is logged to the console, except fatal errors',
                name: '--silent, --reporter silent',
                shortAlias: '-s',
              },
              {
                description: 'The default reporter when the stdout is TTY',
                name: '--reporter default',
              },
              {
                description: 'The output is always appended to the end. No cursor manipulations are performed',
                name: '--reporter append-only',
              },
              {
                description: 'The most verbose reporter. Prints all logs in ndjson format',
                name: '--reporter ndjson',
              },
            ],
          },
          FILTERING,
          {
            title: 'Experimental options',

            list: [
              {
                description: 'Use or cache the results of (pre/post)install hooks',
                name: '--side-effects-cache',
              },
              {
                description: 'Only use the side effects cache if present, do not create it for new packages',
                name: '--side-effects-cache-readonly',
              },
            ],
          },
        ],
        url: 'https://pnpm.js.org/en/cli/install',
        usages: ['pnpm install [options]'],
      })

    case 'add':
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
                name: '--save-exact',
                shortAlias: '-E',
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
              OPTIONS.help,
            ],
          },
        ],
        url: 'https://pnpm.js.org/en/cli/add',
        usages: [
          'pnpm add <name>',
          'pnpm add <name>@<tag>',
          'pnpm add <name>@<version>',
          'pnpm add <name>@<version range>',
          'pnpm add <git host>:<git user>/<repo name>',
          'pnpm add <git repo url>',
          'pnpm add <tarball file>',
          'pnpm add <tarball url>',
          'pnpm add <folder>',
        ],
      })

    case 'import':
      return renderHelp({
        description: `Generates ${WANTED_LOCKFILE} from an npm package-lock.json (or npm-shrinkwrap.json) file.`,
        usages: ['pnpm import'],
      })

    case 'uninstall':
      return renderHelp({
        description: `Aliases: remove, rm, r, un\n\nRemoves packages from \`node_modules\` and from the project's \`packages.json\`.`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Uninstall from every package found in subdirectories
                  or from every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"
                `,
                name: '--recursive',
                shortAlias: '-r',
              }
            ],
          },
        ],
        usages: ['pnpm uninstall <pkg>[@<version>]...'],
      })

    case 'link':
      return renderHelp({
        description: 'Aliases: ln',
        usages: [
          'pnpm link (in package dir)',
          'pnpm link <pkg>',
          'pnpm link <folder>',
        ],
      })

    case 'unlink':
      return renderHelp({
        description: 'Aliases: dislink\n\nRemoves the link created by \`pnpm link\` and reinstalls package if it is saved in \`package.json\`',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Unlink in every package found in subdirectories
                  or in every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
            ],
          },
        ],
        usages: [
          'pnpm unlink (in package dir)',
          'pnpm unlink <pkg>...',
        ],
      })

    case 'update':
      return renderHelp({
        description: 'Aliases: up, upgrade',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`Update in every package found in subdirectories
                  or every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
              {
                description: 'Update globally installed packages',
                name: '--global',
                shortAlias: '-g',
              },
              {
                description: 'How deep should levels of dependencies be inspected. 0 is default, which means top-level dependencies',
                name: '--depth <number>',
              },
              {
                description: 'Ignore version ranges in package.json',
                name: '--latest',
                shortAlias: '-L',
              },
            ],
          },
        ],
        usages: ['pnpm update [-g] [<pkg>...]'],
      })

    case 'list':
      return renderHelp({
        description: 'Aliases: list, la, ll\n\n' + oneLine`When run as ll or la, it shows extended information by default.
          All dependencies are printed by default. Search by patterns is supported.
          For example: pnpm ls babel-* eslint-*`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`Perform command on every package in subdirectories
                  or on every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
              {
                description: 'Show extended information',
                name: '--long',
              },
              {
                description: 'Show parseable output instead of tree view',
                name: '--parseable',
              },
              {
                description: 'Show information in JSON format',
                name: '--json',
              },
              {
                description: 'List packages in the global install prefix instead of in the current project',
                name: '--global',
                shortAlias: '-g',
              },
              {
                description: 'Max display depth of the dependency tree',
                name: '--depth <number>',
              },
              {
                description: 'Display only direct dependencies',
                name: '--depth 0',
              },
              {
                description: 'Display only projects. Useful in a monorepo. \`pnpm ls -r --depth -1\` lists all projects in a monorepo',
                name: '--depth -1',
              },
              {
                description: 'Display only the dependency tree for packages in \`dependencies\`',
                name: '--prod, --production',
              },
              {
                description: 'Display only the dependency tree for packages in \`devDependencies\`',
                name: '--dev',
              },
            ],
          },
        ],
        usages: [
          'pnpm ls [<pkg> ...]',
        ],
      })

    case 'prune':
      return renderHelp({
        description: 'Removes extraneous packages',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: 'Remove the packages specified in \`devDependencies\`',
                name: '--prod, --production',
              },
            ],
          },
        ],
        usages: ['pnpm prune [--production]'],
      })

    case 'pack':
      return renderHelp({
        description: 'Creates a compressed gzip archive of package dependencies.',
        usages: ['pnpm pack'],
      })

    case 'publish':
      return renderHelp({
        description: 'Publishes a package to the npm registry.',
        usages: ['pnpm publish [<tarball>|<folder>] [--tag <tag>] [--access <public|restricted>]'],
      })

    case 'install-test':
      return renderHelp({
        description: 'Aliases: it\n\nRuns a \`pnpm install\` followed immediately by a \`pnpm test\`. It takes exactly the same arguments as \`pnpm install\`.',
        usages: ['pnpm install-test'],
      })

    case 'store':
      return renderHelp({
        description: 'Reads and performs actions on pnpm store that is on the current filesystem.',
        descriptionLists: [
          {
            title: 'Commands',

            list: [
              {
                description: oneLine`
                  Checks for modified packages in the store.
                  Returns exit code 0 if the content of the package is the same as it was at the time of unpacking
                `,
                name: 'status',
              },
              {
                description: 'Adds new packages to the store. Example: pnpm store add express@4 typescript@2.1.0',
                name: 'add <pkg>...',
              },
              {
                description: 'Lists all pnpm projects on the current filesystem that depend on the specified packages. Example: pnpm store usages flatmap-stream',
                name: 'usages <pkg>...',
              },
              {
                description: oneLine`
                  Removes unreferenced (extraneous, orphan) packages from the store.
                  Pruning the store is not harmful, but might slow down future installations.
                  Visit the documentation for more information on unreferenced packages and why they occur
                `,
                name: 'prune',
              },
            ],
          },
        ],
        usages: ['pnpm store <command>'],
      })

    case 'root':
      return renderHelp({
        description: 'Print the effective \`node_modules\` folder.',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: 'Print the global \`node_modules\` folder',
                name: '--global',
                shortAlias: '-g',
              },
            ],
          },
        ],
        usages: ['pnpm root [-g [--independent-leaves]]'],
      })

    case 'outdated':
      return renderHelp({
        description: stripIndent`
          Check for outdated packages. The check can be limited to a subset of the installed packages by providing arguments (patterns are supported).

          Examples:
          pnpm outdated
          pnpm outdated gulp-* @babel/core`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Check for outdated dependencies in every package found in subdirectories
                  or in every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
            ],
          },
        ],
        usages: ['pnpm outdated [<pkg> ...]'],
      })

    case 'rebuild':
      return renderHelp({
        description: 'Aliases: rb\n\nRebuild a package.',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`Rebuild every package found in subdirectories
                  or every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
              {
                description: 'Rebuild packages that were not build during installation. Packages are not build when installing with the --ignore-scripts flag',
                name: '--pending',
              },
            ],
          },
        ],
        usages: ['pnpm rebuild [<pkg> ...]'],
      })

    case 'run':
      return renderHelp({
        description: 'Aliases: run-script\n\nRuns a defined package script.',
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`Run the defined package script in every package found in subdirectories
                  or every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
            ],
          },
        ],
        usages: ['pnpm run <command> [-- <args>...]'],
      })

    case 'test':
      return renderHelp({
        description: `Aliases: t, tst\n\nRuns a package's "test" script, if one was provided.`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Run the tests in every package found in subdirectories
                  or every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
            ],
          },
        ],
        usages: ['pnpm test [-- <args>...]'],
      })

    case 'start':
      return renderHelp({
        description: oneLine`
          Runs an arbitrary command specified in the package's "start" property of its "scripts" object.
          If no "start" property is specified on the "scripts" object, it will run node server.js.`,
        usages: ['pnpm start [-- <args>...]'],
      })

    case 'stop':
      return renderHelp({
        description: `Runs a package's "stop" script, if one was provided.`,
        usages: ['pnpm stop [-- <args>...]'],
      })

    case 'restart':
      return renderHelp({
        description: `Restarts a package. Runs a package's "stop", "restart", and "start" scripts, and associated pre- and post- scripts.`,
        usages: ['pnpm restart [-- <args>...]'],
      })

    case 'server':
      return renderHelp({
        description: 'Manage a store server',
        descriptionLists: [
          {
            title: 'Commands',

            list: [
              {
                description: oneLine`
                  Starts a service that does all interactions with the store.
                  Other commands will delegate any store-related tasks to this service`,
                name: 'start',
              },
              {
                description: 'Stops the store server',
                name: 'stop',
              },
              {
                description: 'Prints information about the running server',
                name: 'status',
              },
            ],
          },
          {
            title: 'Start options',

            list: [
              {
                description: 'Runs the server in the background',
                name: '--background',
              },
              {
                description: 'The communication protocol used by the server',
                name: '--protocol <auto|tcp|ipc>',
              },
              {
                description: 'The port number to use, when TCP is used for communication',
                name: '--port <number>',
              },
              {
                description: 'The location where all the packages are saved on the disk',
                name: '--store',
              },
              {
                description: 'Maximum number of concurrent network requests',
                name: '--network-concurrency <number>',
              },
              {
                description: "If false, doesn't check whether packages in the store were mutated",
                name: '--[no-]verify-store-integrity',
              },
              {
                description: '',
                name: '--[no-]lock',
              },
              {
                description: 'Disallows stopping the server using \`pnpm server stop\`',
                name: '--ignore-stop-requests',
              },
              {
                description: 'Disallows creating new side effect cache during install',
                name: '--ignore-upload-requests',
              },
            ],
          },
        ],
        usages: ['pnpm server <command>'],
      })

    case 'recursive':
      return stripIndent`
        pnpm recursive [command] [flags] [-- <package selector>...]
        pnpm multi [command] [flags] [-- <package selector>...]
        pnpm m [command] [flags] [-- <package selector>...]

        Concurrently performs some actions in all subdirectories with a \`package.json\` (excluding node_modules).
        A \`pnpm-workspace.yaml\` file may be used to control what directories are searched for packages.

        Commands:

          install

          add

          update

          uninstall <pkg>...
            Uninstall a dependency from each package

          unlink
            Removes links to local packages and reinstalls them from the registry.

          list [<pkg>...]
            List dependencies in each package.

          outdated [<pkg>...]
            Check for outdated dependencies in every package.

          run <command> [-- <args>...]
            This runs an arbitrary command from each package's "scripts" object.
            If a package doesn't have the command, it is skipped.
            If none of the packages have the command, the command fails.

          test [-- <args>...]
            This runs each package's "test" script, if one was provided.

          rebuild [[<@scope>/<name>]...]
            This command runs the "npm build" command on each package.
            This is useful when you install a new version of node,
            and must recompile all your C++ addons with the new binary.

          exec -- <command> [args...]      run a command in each package.

        Options:

          -- <package selector>..., --filter <package selector>
            Run the command only on packages that satisfy at least one of the selectors.

            Example: pnpm recursive install -- foo... ...@bar/* qar ./components

            These selectors may be used:

            <pattern>
              Restricts the scope to package names matching the given pattern. E.g.: foo, @bar/*

            <pattern>...
              Includes all direct and indirect dependencies of the matched packages. E.g.: foo...

            ...<pattern>
              Includes all direct and indirect dependents of the matched packages. E.g.: ...foo, ...@bar/*

            ./<directory>
              Includes all packages that are inside a given subdirectory. E.g.: ./components

            .
              Includes all packages that are under the current working directory.

          --no-bail
            Continues executing other tasks even if a task threw an error.

          --workspace-concurrency <number>
            Set the maximum number of concurrency. Default is 4. For unlimited concurrency use Infinity.

          --link-workspace-packages
            Locally available packages are linked to node_modules instead of being downloaded from the registry.
            Convenient to use in a multi-package repository.

          --sort
            Sort packages topologically (dependencies before dependents). Pass --no-sort to disable.

          --shared-workspace-lockfile
            Creates a single ${WANTED_LOCKFILE} file in the root of the workspace.
            A shared lockfile also means that all dependencies of all workspace packages will be in a single node_modules.
      `

    default:
      return renderHelp({
        description: '',
        descriptionLists: [
          {
            title: 'Manage your dependencies',

            list: [
              {
                description: '',
                name: 'install',
                shortAlias: 'i',
              },
              {
                description: '',
                name: 'add',
                shortAlias: '',
              },
              {
                description: '',
                name: 'update',
                shortAlias: 'up',
              },
              {
                description: '',
                name: 'remove',
                shortAlias: 'rm',
              },
              {
                description: '',
                name: 'link',
                shortAlias: 'ln',
              },
              {
                description: '',
                name: 'unlink',
                shortAlias: '',
              },
              {
                description: '',
                name: 'import',
                shortAlias: '',
              },
              {
                description: '',
                name: 'install-test',
                shortAlias: 'it',
              },
              {
                description: '',
                name: 'rebuild',
                shortAlias: 'rb',
              },
              {
                description: '',
                name: 'prune',
                shortAlias: '',
              },
            ],
          },
          {
            title: 'Review your dependencies',

            list: [
              {
                description: '',
                name: 'list',
                shortAlias: 'ls',
              },
              {
                description: '',
                name: 'outdated',
              },
            ],
          },
          {
            title: 'Run your scripts',

            list: [
              {
                description: '',
                name: 'run',
              },
              {
                description: '',
                name: 'test',
                shortAlias: 't',
              },
              {
                description: '',
                name: 'start',
              },
              {
                description: '',
                name: 'restart',
              },
              {
                description: '',
                name: 'stop',
              },
            ],
          },
          {
            title: 'Other',

            list: [
              {
                description: '',
                name: 'pack',
              },
              {
                description: '',
                name: 'publish',
              },
              {
                description: '',
                name: 'root',
              },
            ],
          },
          {
            title: 'Manage you monorepo',

            list: [
              {
                description: '',
                name: 'recursive exec',
              },
              {
                description: '',
                name: 'recursive install',
              },
              {
                description: '',
                name: 'recursive add',
              },
              {
                description: '',
                name: 'recursive list',
              },
              {
                description: '',
                name: 'recursive outdated',
              },
              {
                description: '',
                name: 'recursive rebuild',
              },
              {
                description: '',
                name: 'recursive run',
              },
              {
                description: '',
                name: 'recursive test',
              },
              {
                description: '',
                name: 'recursive uninstall',
              },
              {
                description: '',
                name: 'recursive unlink',
              },
              {
                description: '',
                name: 'recursive update',
              },
            ],
          },
          {
            title: 'Use a store server',

            list: [
              {
                description: '',
                name: 'server start',
              },
              {
                description: '',
                name: 'server status',
              },
              {
                description: '',
                name: 'server stop',
              },
            ],
          },
          {
            title: 'Manage your store',

            list: [
              {
                description: '',
                name: 'store add',
              },
              {
                description: '',
                name: 'store prune',
              },
              {
                description: '',
                name: 'store status',
              },
            ],
          },
        ],
        usages: ['pnpm [command] [flags]', 'pnpm [ -h | --help | -v | --version ]'],
      })
  }
}
