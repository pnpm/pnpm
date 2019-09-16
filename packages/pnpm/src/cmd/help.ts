import { WANTED_LOCKFILE } from '@pnpm/constants'
import { oneLine, stripIndent } from 'common-tags'
import R = require('ramda')
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
    full: '--help',
    short: '-h',
  },
  ignoreScripts: {
    description: "Don't run lifecycle scripts",
    full: '--ignore-scripts',
  },
  offline: {
    description: 'Trigger an error if any required dependencies are not available in local store',
    full: '--offline',
  },
  preferOffline: {
    description: 'Skip staleness checks for cached data, but request missing data from the server',
    full: '--prefer-offline',
  },
}
const FILTERING = {
  options: [
    {
      description: 'Restricts the scope to package names matching the given pattern. E.g.: foo, @bar/*',
      full: '--filter <pattern>',
    },
    {
      description: 'Includes all direct and indirect dependencies of the matched packages. E.g.: foo...',
      full: '--filter <pattern>...',
    },
    {
      description: 'Includes all direct and indirect dependents of the matched packages. E.g.: ...foo, ...@bar/*',
      full: '--filter ...<pattern>',
    },
    {
      description: 'Includes all packages that are inside a given subdirectory. E.g.: ./components',
      full: '--filter ./<directory>',
    },
    {
      description: 'Includes all packages that are under the current working directory',
      full: '--filter .',
    },
  ],
  title: 'Filtering options (run the command only on packages that satisfy at least one of the selectors)',
}

type OptionInfo = { short?: string, full: string, description: string }

function renderHelp (
  config: {
    options?: OptionInfo[],
    groupedOptions?: Array<{
      title: string,
      options: OptionInfo[],
    }>,
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
  if (config.options) {
    output += `Options:\n` + renderOptions(config.options)
  }
  if (config.groupedOptions) {
    for (let { title, options } of config.groupedOptions) {
      output += `${title}:\n` + renderOptions(options)
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

function renderOptions (options: OptionInfo[]) {
  const data = options.map(({ short, full, description }) => [short && `${short},` || '', full, description])
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
        groupedOptions: [
          {
            title: 'Output',

            options: [
              {
                description: 'No output is logged to the console, except fatal errors',
                full: '--silent, --reporter silent',
                short: '-s',
              },
              {
                description: 'The default reporter when the stdout is TTY',
                full: '--reporter default',
              },
              {
                description: 'The output is always appended to the end. No cursor manipulations are performed',
                full: '--reporter append-only',
              },
              {
                description: 'The most verbose reporter. Prints all logs in ndjson format',
                full: '--reporter ndjson',
              },
            ],
          },
          FILTERING,
          {
            title: 'Experimental options',

            options: [
              {
                description: 'Use or cache the results of (pre/post)install hooks',
                full: '--side-effects-cache',
              },
              {
                description: 'Only use the side effects cache if present, do not create it for new packages',
                full: '--side-effects-cache-readonly',
              },
            ],
          },
        ],
        options: [
          {
            description: oneLine`
              Run installation recursively in every package found in subdirectories.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          {
            description: "Packages in \`devDependencies\` won't be installed",
            full: '--production, --only prod[uction]',
          },
          {
            description: 'Only \`devDependencies\` are installed regardless of the \`NODE_ENV\`',
            full: '--only dev[elopment]',
          },
          {
            description: `Don't read or generate a \`${WANTED_LOCKFILE}\` file`,
            full: '--no-lockfile',
          },
          {
            description: `Dependencies are not downloaded. Only \`${WANTED_LOCKFILE}\` is updated`,
            full: '--lockfile-only',
          },
          {
            description: "Don't generate a lockfile and fail if an update is needed",
            full: '--frozen-lockfile',
          },
          {
            description: `If the available \`${WANTED_LOCKFILE}\` satisfies the \`package.json\` then perform a headless installation`,
            full: '--prefer-frozen-lockfile',
          },
          {
            description: `The directory in which the ${WANTED_LOCKFILE} of the package will be created. Several projects may share a single lockfile`,
            full: '--lockfile-directory <path>',
          },
          {
            description: 'Dependencies inside node_modules have access only to their listed dependencies',
            full: '--no-hoist',
          },
          {
            description: 'The subdeps will be hoisted into the root node_modules. Your code will have access to them',
            full: '--shamefully-hoist',
          },
          {
            description: 'Hoist all dependencies matching the pattern to the root of node_modules. Supplying it a * will hoist all dependencies (this is similar to what npm does)',
            full: '--hoist-pattern <pattern>',
          },
          {
            description: 'The location where all the packages are saved on the disk',
            full: '--store <path>',
          },
          {
            description: 'Maximum number of concurrent network requests',
            full: '--network-concurrency <number>',
          },
          {
            description: 'Controls the number of child processes run parallelly to build node modules',
            full: '--child-concurrency <number>',
          },
          {
            description: 'Disable pnpm hooks defined in pnpmfile.js',
            full: '--ignore-pnpmfile',
          },
          {
            description: 'Symlinks leaf dependencies directly from the global store',
            full: '--independent-leaves',
          },
          {
            description: "If false, doesn't check whether packages in the store were mutated",
            full: '--[no-]verify-store-integrity',
          },
          {
            description: '',
            full: '--[no-]lock',
          },
          {
            description: 'Fail on missing or invalid peer dependencies',
            full: '--strict-peer-dependencies',
          },
          {
            description: 'Starts a store server in the background. The store server will keep running after installation is done. To stop the store server, run \`pnpm server stop\`',
            full: '--use-store-server',
          },
          {
            description: 'Only allows installation with a store server. If no store server is running, installation will fail',
            full: '--use-running-store-server',
          },
          {
            description: 'Try to hardlink packages from the store. If it fails, fallback to copy',
            full: '--package-import-method auto',
          },
          {
            description: 'Hardlink packages from the store',
            full: '--package-import-method hardlink',
          },
          {
            description: 'Copy packages from the store',
            full: '--package-import-method copy',
          },
          {
            description: 'Reflink (aka copy-on-write) packages from the store',
            full: '--package-import-method reflink',
          },
          {
            description: 'The default resolution strategy. Speed is preferred over deduplication',
            full: '--resolution-strategy fast',
          },
          {
            description: 'Already installed dependencies are preferred even if newer versions satisfy a range',
            full: '--resolution-strategy fewer-dependencies',
          },
          OPTIONS.help,
        ],
        url: 'https://pnpm.js.org/en/cli/install',
        usages: ['pnpm install [options]'],
      })

    case 'add':
      return renderHelp({
        description: 'Installs a package and any packages that it depends on.',
        options: [
          {
            description: 'Save package to your \`dependencies\`. The default behavior',
            full: '--save-prod',
            short: '-P',
          },
          {
            description: 'Save package to your \`devDependencies\`',
            full: '--save-dev',
            short: '-D',
          },
          {
            description: 'Save package to your \`optionalDependencies\`',
            full: '--save-optional',
            short: '-O',
          },
          {
            description: 'Save package to your \`peerDependencies\` and \`devDependencies\`',
            full: '--save-peer',
          },
          {
            description: 'Install exact version',
            full: '--save-exact',
            short: '-E',
          },
          {
            description: 'Install as a global package',
            full: '--global',
            short: '-g',
          },
          {
            description: oneLine`Run installation recursively in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.help,
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
        options: [
          {
            description: oneLine`
              Uninstall from every package found in subdirectories
              or from every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"
            `,
            full: '--recursive',
            short: '-r',
          }
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
        options: [
          {
            description: oneLine`
              Unlink in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
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
        options: [
          {
            description: oneLine`Update in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
          {
            description: 'Update globally installed packages',
            full: '--global',
            short: '-g',
          },
          {
            description: 'How deep should levels of dependencies be inspected. 0 is default, which means top-level dependencies',
            full: '--depth <number>',
          },
          {
            description: 'Ignore version ranges in package.json',
            full: '--latest',
            short: '-L',
          },
        ],
        usages: ['pnpm update [-g] [<pkg>...]'],
      })

    case 'list':
      return renderHelp({
        description: 'Aliases: list, la, ll\n\n' + oneLine`When run as ll or la, it shows extended information by default.
          All dependencies are printed by default. Search by patterns is supported.
          For example: pnpm ls babel-* eslint-*`,
        options: [
          {
            description: oneLine`Perform command on every package in subdirectories
              or on every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
          {
            description: 'Show extended information',
            full: '--long',
          },
          {
            description: 'Show parseable output instead of tree view',
            full: '--parseable',
          },
          {
            description: 'Show information in JSON format',
            full: '--json',
          },
          {
            description: 'List packages in the global install prefix instead of in the current project',
            full: '--global',
            short: '-g',
          },
          {
            description: 'Max display depth of the dependency tree',
            full: '--depth <number>',
          },
          {
            description: 'Display only direct dependencies',
            full: '--depth 0',
          },
          {
            description: 'Display only projects. Useful in a monorepo. \`pnpm ls -r --depth -1\` lists all projects in a monorepo',
            full: '--depth -1',
          },
          {
            description: 'Display only the dependency tree for packages in \`dependencies\`',
            full: '--prod, --production',
          },
          {
            description: 'Display only the dependency tree for packages in \`devDependencies\`',
            full: '--dev',
          },
        ],
        usages: [
          'pnpm ls [<pkg> ...]',
        ],
      })

    case 'prune':
      return renderHelp({
        description: 'Removes extraneous packages',
        options: [
          {
            description: 'Remove the packages specified in \`devDependencies\`',
            full: '--prod, --production',
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
        groupedOptions: [
          {
            title: 'Commands',

            options: [
              {
                description: oneLine`
                  Checks for modified packages in the store.
                  Returns exit code 0 if the content of the package is the same as it was at the time of unpacking
                `,
                full: 'status',
              },
              {
                description: 'Adds new packages to the store. Example: pnpm store add express@4 typescript@2.1.0',
                full: 'add <pkg>...',
              },
              {
                description: 'Lists all pnpm projects on the current filesystem that depend on the specified packages. Example: pnpm store usages flatmap-stream',
                full: 'usages <pkg>...',
              },
              {
                description: oneLine`
                  Removes unreferenced (extraneous, orphan) packages from the store.
                  Pruning the store is not harmful, but might slow down future installations.
                  Visit the documentation for more information on unreferenced packages and why they occur
                `,
                full: 'prune',
              },
            ],
          },
        ],
        usages: ['pnpm store <command>'],
      })

    case 'root':
      return renderHelp({
        description: 'Print the effective \`node_modules\` folder.',
        options: [
          {
            description: 'Print the global \`node_modules\` folder',
            full: '--global',
            short: '-g',
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
        options: [
          {
            description: oneLine`
              Check for outdated dependencies in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
        ],
        usages: ['pnpm outdated [<pkg> ...]'],
      })

    case 'rebuild':
      return renderHelp({
        description: 'Aliases: rb\n\nRebuild a package.',
        options: [
          {
            description: oneLine`Rebuild every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
          {
            description: 'Rebuild packages that were not build during installation. Packages are not build when installing with the --ignore-scripts flag',
            full: '--pending',
          },
        ],
        usages: ['pnpm rebuild [<pkg> ...]'],
      })

    case 'run':
      return renderHelp({
        description: 'Aliases: run-script\n\nRuns a defined package script.',
        options: [
          {
            description: oneLine`Run the defined package script in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
          },
        ],
        usages: ['pnpm run <command> [-- <args>...]'],
      })

    case 'test':
      return renderHelp({
        description: `Aliases: t, tst\n\nRuns a package's "test" script, if one was provided.`,
        options: [
          {
            description: oneLine`
              Run the tests in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            full: '--recursive',
            short: '-r',
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
        groupedOptions: [
          {
            title: 'Commands',

            options: [
              {
                description: oneLine`
                  Starts a service that does all interactions with the store.
                  Other commands will delegate any store-related tasks to this service`,
                full: 'start',
              },
              {
                description: 'Stops the store server',
                full: 'stop',
              },
              {
                description: 'Prints information about the running server',
                full: 'status',
              },
            ],
          },
          {
            title: 'Start options',

            options: [
              {
                description: 'Runs the server in the background',
                full: '--background',
              },
              {
                description: 'The communication protocol used by the server',
                full: '--protocol <auto|tcp|ipc>',
              },
              {
                description: 'The port number to use, when TCP is used for communication',
                full: '--port <number>',
              },
              {
                description: 'The location where all the packages are saved on the disk',
                full: '--store',
              },
              {
                description: 'Maximum number of concurrent network requests',
                full: '--network-concurrency <number>',
              },
              {
                description: "If false, doesn't check whether packages in the store were mutated",
                full: '--[no-]verify-store-integrity',
              },
              {
                description: '',
                full: '--[no-]lock',
              },
              {
                description: 'Disallows stopping the server using \`pnpm server stop\`',
                full: '--ignore-stop-requests',
              },
              {
                description: 'Disallows creating new side effect cache during install',
                full: '--ignore-upload-requests',
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
        groupedOptions: [
          {
            title: 'Manage your dependencies',

            options: [
              {
                description: '',
                full: 'install',
                short: 'i',
              },
              {
                description: '',
                full: 'add',
                short: '',
              },
              {
                description: '',
                full: 'update',
                short: 'up',
              },
              {
                description: '',
                full: 'remove',
                short: 'rm',
              },
              {
                description: '',
                full: 'link',
                short: 'ln',
              },
              {
                description: '',
                full: 'unlink',
                short: '',
              },
              {
                description: '',
                full: 'import',
                short: '',
              },
              {
                description: '',
                full: 'install-test',
                short: 'it',
              },
              {
                description: '',
                full: 'rebuild',
                short: 'rb',
              },
              {
                description: '',
                full: 'prune',
                short: '',
              },
            ],
          },
          {
            title: 'Review your dependencies',

            options: [
              {
                description: '',
                full: 'list',
                short: 'ls',
              },
              {
                description: '',
                full: 'outdated',
              },
            ],
          },
          {
            title: 'Run your scripts',

            options: [
              {
                description: '',
                full: 'run',
              },
              {
                description: '',
                full: 'test',
                short: 't',
              },
              {
                description: '',
                full: 'start',
              },
              {
                description: '',
                full: 'restart',
              },
              {
                description: '',
                full: 'stop',
              },
            ],
          },
          {
            title: 'Other',

            options: [
              {
                description: '',
                full: 'pack',
              },
              {
                description: '',
                full: 'publish',
              },
              {
                description: '',
                full: 'root',
              },
            ],
          },
          {
            title: 'Manage you monorepo',

            options: [
              {
                description: '',
                full: 'recursive exec',
              },
              {
                description: '',
                full: 'recursive install',
              },
              {
                description: '',
                full: 'recursive add',
              },
              {
                description: '',
                full: 'recursive list',
              },
              {
                description: '',
                full: 'recursive outdated',
              },
              {
                description: '',
                full: 'recursive rebuild',
              },
              {
                description: '',
                full: 'recursive run',
              },
              {
                description: '',
                full: 'recursive test',
              },
              {
                description: '',
                full: 'recursive uninstall',
              },
              {
                description: '',
                full: 'recursive unlink',
              },
              {
                description: '',
                full: 'recursive update',
              },
            ],
          },
          {
            title: 'Use a store server',

            options: [
              {
                description: '',
                full: 'server start',
              },
              {
                description: '',
                full: 'server status',
              },
              {
                description: '',
                full: 'server stop',
              },
            ],
          },
          {
            title: 'Manage your store',

            options: [
              {
                description: '',
                full: 'store add',
              },
              {
                description: '',
                full: 'store prune',
              },
              {
                description: '',
                full: 'store status',
              },
            ],
          },
        ],
        usages: ['pnpm [command] [flags]', 'pnpm [ -h | --help | -v | --version ]'],
      })
  }
}
