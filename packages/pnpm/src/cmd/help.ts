import { WANTED_LOCKFILE } from '@pnpm/constants'
import { oneLine, stripIndent } from 'common-tags'
import renderHelp = require('render-help')
import getCommandFullName from '../getCommandFullName'
import pnpmPkgJson from '../pnpmPkgJson'

export default function (input: string[]) {
  const cmdName = getCommandFullName(input[0])
  console.log(`Version ${pnpmPkgJson.version}\n${getHelpText(cmdName)}`)
}

const docsUrl = (cmd: string) => `https://pnpm.js.org/en/cli/${cmd}`

const OPTIONS = {
  color: {
    description: 'Controls colors in the output. By default, output is always colored when it goes directly to a terminal',
    name: '--[no-]color',
  },
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
  store: {
    description: 'The location where all the packages are saved on the disk',
    name: '--store <path>',
  },
  virtualStoreDir: {
    description: 'The directory that contains all the dependencies that are linked from the store (default is node_modules/.pnpm)',
    name: '--virtual-store-dir',
  }
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

function getHelpText (command: string) {
  switch (getCommandFullName(command)) {
    case 'install':
      return renderHelp({
        aliases: ['i'],
        description: oneLine`Installs all dependencies of the project in the current working directory.
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
                description: oneLine`
                  Hoist all dependencies matching the pattern to \`node_modules/.pnpm/node_modules\`.
                  The default pattern is * and matches everything. Hoisted packages can be required
                  by any dependencies, so it is an emulation of a flat node_modules`,
                name: '--hoist-pattern <pattern>',
              },
              OPTIONS.store,
              OPTIONS.virtualStoreDir,
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
                description: 'Clones/hardlinks or copies packages. The selected method depends from the file system',
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
                description: 'Clone (aka copy-on-write) packages from the store',
                name: '--package-import-method clone',
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
              OPTIONS.color,
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
        url: docsUrl(command),
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
              OPTIONS.help,
              OPTIONS.color,
              OPTIONS.store,
              OPTIONS.virtualStoreDir,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
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
        url: docsUrl(command),
        usages: ['pnpm import'],
      })

    case 'remove':
      return renderHelp({
        aliases: ['rm', 'r', 'uninstall', 'un'],
        description: `Removes packages from \`node_modules\` and from the project's \`packages.json\`.`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                  Remove from every package found in subdirectories
                  or from every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"
                `,
                name: '--recursive',
                shortAlias: '-r',
              },
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl('remove'),
        usages: ['pnpm remove <pkg>[@<version>]...'],
      })

    case 'link':
      return renderHelp({
        aliases: ['ln'],
        descriptionLists: [
          {
            title: 'Options',

            list: [
              OPTIONS.color,
            ],
          },
        ],
        url: docsUrl(command),
        usages: [
          'pnpm link (in package dir)',
          'pnpm link <pkg>',
          'pnpm link <folder>',
        ],
      })

    case 'unlink':
      return renderHelp({
        aliases: ['dislink'],
        description: 'Removes the link created by \`pnpm link\` and reinstalls package if it is saved in \`package.json\`',
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
              OPTIONS.color,
            ],
          },
        ],
        url: docsUrl(command),
        usages: [
          'pnpm unlink (in package dir)',
          'pnpm unlink <pkg>...',
        ],
      })

    case 'update':
      return renderHelp({
        aliases: ['up', 'upgrade'],
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
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: ['pnpm update [-g] [<pkg>...]'],
      })

    case 'list':
      return renderHelp({
        aliases: ['list', 'ls', 'la', 'll'],
        description: oneLine`When run as ll or la, it shows extended information by default.
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
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: [
          'pnpm ls [<pkg> ...]',
        ],
      })

    case 'why':
      return renderHelp({
        description: stripIndent`
          Shows the packages that depend on <pkg>
          For example: pnpm why babel-* eslint-*`,
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
                description: 'Display only the dependency tree for packages in \`dependencies\`',
                name: '--prod, --production',
              },
              {
                description: 'Display only the dependency tree for packages in \`devDependencies\`',
                name: '--dev',
              },
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: [
          'pnpm why <pkg> ...',
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
              OPTIONS.color,
            ],
          },
        ],
        url: docsUrl(command),
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
        url: docsUrl(command),
        usages: ['pnpm publish [<tarball>|<folder>] [--tag <tag>] [--access <public|restricted>]'],
      })

    case 'install-test':
      return renderHelp({
        aliases: ['it'],
        description: 'Runs a \`pnpm install\` followed immediately by a \`pnpm test\`. It takes exactly the same arguments as \`pnpm install\`.',
        url: docsUrl(command),
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
        url: docsUrl(command),
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
          pnpm outdated --long
          pnpm outdated gulp-* @babel/core`,
        descriptionLists: [
          {
            title: 'Options',

            list: [
              {
                description: oneLine`
                By default, details about the outdated packages (such as a link to the repo) are not displayed.
                To display the details, pass this option.`,
                name: '--long'
              },
              {
                description: oneLine`
                  Check for outdated dependencies in every package found in subdirectories
                  or in every workspace package, when executed inside a workspace.
                  For options that may be used with \`-r\`, see "pnpm help recursive"`,
                name: '--recursive',
                shortAlias: '-r',
              },
              {
                description: 'Prints the outdated packages in a list. Good for small consoles',
                name: '--no-table',
              },
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: ['pnpm outdated [<pkg> ...]'],
      })

    case 'rebuild':
      return renderHelp({
        aliases: ['rb'],
        description: 'Rebuild a package.',
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
              OPTIONS.color,
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: ['pnpm rebuild [<pkg> ...]'],
      })

    case 'run':
      return renderHelp({
        aliases: ['run-script'],
        description: 'Runs a defined package script.',
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
          FILTERING,
        ],
        url: docsUrl(command),
        usages: ['pnpm run <command> [-- <args>...]'],
      })

    case 'test':
      return renderHelp({
        aliases: ['t', 'tst'],
        description: `Runs a package's "test" script, if one was provided.`,
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
          FILTERING,
        ],
        url: docsUrl(command),
        usages: ['pnpm test [-- <args>...]'],
      })

    case 'start':
      return renderHelp({
        description: oneLine`
          Runs an arbitrary command specified in the package's "start" property of its "scripts" object.
          If no "start" property is specified on the "scripts" object, it will run node server.js.`,
        url: docsUrl(command),
        usages: ['pnpm start [-- <args>...]'],
      })

    case 'stop':
      return renderHelp({
        description: `Runs a package's "stop" script, if one was provided.`,
        url: docsUrl(command),
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
              OPTIONS.color,
            ],
          },
        ],
        url: docsUrl(command),
        usages: ['pnpm server <command>'],
      })

    case 'recursive':
      return renderHelp({
        description: oneLine`
          Concurrently performs some actions in all subdirectories with a \`package.json\` (excluding node_modules).
          A \`pnpm-workspace.yaml\` file may be used to control what directories are searched for packages.`,
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
                description: oneLine`
                  This runs an arbitrary command from each package's "scripts" object.
                  If a package doesn't have the command, it is skipped.
                  If none of the packages have the command, the command fails.`,
                name: 'run <command> [-- <args>...]',
              },
              {
                description: `This runs each package's "test" script, if one was provided.`,
                name: 'test [-- <args>...]',
              },
              {
                description: oneLine`
                  This command runs the "npm build" command on each package.
                  This is useful when you install a new version of node,
                  and must recompile all your C++ addons with the new binary.`,
                name: 'rebuild [[<@scope>/<name>]...]',
              },
              {
                description: `Run a command in each package.`,
                name: 'exec -- <command> [args...]',
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
                description: oneLine`
                  Locally available packages are linked to node_modules instead of being downloaded from the registry.
                  Convenient to use in a multi-package repository.`,
                name: '--link-workspace-packages',
              },
              {
                description: 'Sort packages topologically (dependencies before dependents). Pass --no-sort to disable.',
                name: '--sort',
              },
              {
                description: oneLine`
                  Creates a single ${WANTED_LOCKFILE} file in the root of the workspace.
                  A shared lockfile also means that all dependencies of all workspace packages will be in a single node_modules.`,
                name: '--shared-workspace-lockfile',
              },
            ],
          },
          FILTERING,
        ],
        url: docsUrl(command),
        usages: [
          'pnpm recursive [command] [flags] [--filter <package selector>]',
          'pnpm multi [command] [flags] [--filter <package selector>]',
          'pnpm m [command] [flags] [--filter <package selector>]'
        ],
      })

    default:
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
}
