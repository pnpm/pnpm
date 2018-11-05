import { stripIndent } from 'common-tags'
import getCommandFullName from '../getCommandFullName'

export default function (input: string[]) {
  const cmdName = getCommandFullName(input[0])
  console.log(getHelpText(cmdName))
}

function getHelpText (command: string) {
  switch (getCommandFullName(command)) {
    case 'install':
      return stripIndent`
        pnpm install (with no args, in package dir)
        pnpm install [<@scope>/]<name>
        pnpm install [<@scope>/]<name>@<tag>
        pnpm install [<@scope>/]<name>@<version>
        pnpm install [<@scope>/]<name>@<version range>
        pnpm install <git-host>:<git-user>/<repo-name>
        pnpm install <git repo url>
        pnpm install <tarball file>
        pnpm install <tarball url>
        pnpm install <folder>

        Aliases: i, install, add

        Installs all dependencies of the project in the current working directory.
        To install dependencies in every project of a monorepo, run \`pnpm recursive install\`
        or \`pnpm install\` with filtering. For instance, \`pnpm install -- .\`.

        Options:

          -P, --save-prod                    save package to your \`dependencies\`
          -D, --save-dev                     save package to your \`devDependencies\`
          -O, --save-optional                save package to your \`optionalDependencies\`
          -E, --save-exact                   install exact version
          -g, --global                       install as a global package
          --store                            the location where all the packages are saved on the disk.
          --offline                          trigger an error if any required dependencies are not available in local store
          --prefer-offline                   skip staleness checks for cached data, but request missing data from the server
          --network-concurrency <number>     maximum number of concurrent network requests
          --child-concurrency <number>       controls the number of child processes run parallelly to build node modules
          --ignore-pnpmfile                  disable pnpm hooks defined in pnpmfile.js
          --independent-leaves               symlinks leaf dependencies directly from the global store
          --[no-]verify-store-integrity      if false, doesn't check whether packages in the store were mutated
          --production, --only prod[uction]  packages in \`devDependencies\` won't be installed
          --only dev[elopment]               only \`devDependencies\` are installed regardless of the \`NODE_ENV\`.
          --[no-]lock
          --strict-peer-dependencies         fail on missing or invalid peer dependencies.

          --no-shrinkwrap                    don't read or generate a \`shrinkwrap.yaml\` file
          --shrinkwrap-only                  dependencies are not downloaded only \`shrinkwrap.yaml\` is updated
          --frozen-shrinkwrap                don't generate a shrinkwrap file and fail if an update is needed
          --prefer-frozen-shrinkwrap         if the available \`shrinkwrap.yaml\` satisfies the \`package.json\`
                                             then perform a headless installation.
          --shrinkwrap-directory <path>      the directory in which the shrinkwrap.yaml of the package will be created.
                                             Several projects may share a single shrinkwrap file.

          --use-store-server                 starts a store server in the background.
                                             The store server will keep running after installation is done.
                                             To stop the store server, run \`pnpm server stop\`

          --use-running-store-server         only allows installation with a store server. If no store server
                                             is running, installation will fail.

          --package-import-method auto       try to hardlink packages from the store. If it fails, fallback to copy
          --package-import-method hardlink   hardlink packages from the store
          --package-import-method copy       copy packages from the store
          --package-import-method reflink    reflink (aka copy-on-write) packages from the store

          -s, --silent, --reporter silent    no output is logged to the console, except fatal errors
          --reporter default                 the default reporter when the stdout is TTY
          --reporter append-only             the output is always appended to the end. No cursor manipulations are performed
          --reporter ndjson                  the most verbose reporter. Prints all logs in ndjson format

        Experimental options:
          --side-effects-cache               use or cache the results of (pre/post)install hooks
          --side-effects-cache-readonly      only use the side effects cache if present, do not create it for new packages

        Discouraged options:
          --shamefully-flatten               Attempt to flatten the dependency tree, similar to what npm does

        Filtering options:
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
      `

    case 'import':
      return stripIndent`
        pnpm import

        Generates shrinkwrap.yaml from an npm package-lock.json (or npm-shrinkwrap.json) file.
      `

    case 'uninstall':
      return stripIndent`
        pnpm uninstall [<@scope>/]<pkg>[@<version>]...

        Aliases: remove, rm, r, un, unlink

        Removes packages from \`node_modules\` and from the project's \`packages.json\`

        Discouraged options:
          --shamefully-flatten           Attempt to flatten the dependency tree, similar to what npm does
      `

    case 'link':
      return stripIndent`
        pnpm link (in package dir)
        pnpm link [<@scope>/]<pkg>
        pnpm link <folder>

        Aliases: ln
      `

    case 'unlink':
      return stripIndent`
        pnpm unlink (in package dir)
        pnpm unlink [<@scope>/]<pkg>...

        Aliases: dislink

        Removes the link created by \`pnpm link\` and reinstalls package if it is saved in \`package.json\`
      `

    case 'update':
      return stripIndent`
        pnpm update [-g] [<pkg>...]

        Aliases: up, upgrade

        Options:

          -g, --global                    update globally installed packages
          --depth                         how deep should levels of dependencies be inspected
                                          0 is default, which means top-level dependencies

        Discouraged options:
          --shamefully-flatten            Attempt to flatten the dependency tree, similar to what npm does
      `

    case 'list':
      return stripIndent`
        pnpm ls [[<@scope>/]<pkg> ...]

        Aliases: list, la, ll

        When run as ll or la, it shows extended information by default.

        Options:

          --long                          show extended information
          --parseable                     show parseable output instead of tree view
          -g, --global                    list packages in the global install prefix instead of in the current project
          --depth                         max display depth of the dependency tree
          --prod, --production            display only the dependency tree for packages in \`dependencies\`.
          --dev                           display only the dependency tree for packages in \`devDependencies\`.
      `

    case 'prune':
      return stripIndent`
        pnpm prune [--production]

        Options:

          --prod, --production            remove the packages specified in \`devDependencies\`

        Removes extraneous packages
      `

    case 'install-test':
      return stripIndent`
        This command runs an \`npm install\` followed immediately by an \`npm test\`.
        It takes exactly the same arguments as \`npm install\`.
      `

    case 'store':
      return stripIndent`
        pnpm store status

        Returns a 0 exit code if packages in the store are not modified, i.e. the content of the package is the
        same as it was at the time of unpacking.

        pnpm store add [pkg...]

        Adds new packages to the store. E.g.: pnpm store add express@4 typescript@2

        pnpm store prune

        Removes unreferenced (extraneous, orphan) packages from the store. Unreferenced packages are packages that are not used by
        any projects on the system. Packages can become unreferenced after most installation operations. For instance, package
        foo@1.0.0 is updated to foo@1.0.1. If package foo@1.0.0 is not used by any other project on the system, it becomes unreferenced.

        It is good to keep unreferenced packages in the store for a while because frequently unreferenced packages are again needed
        very soon. For instance, after changing branch on a project and installing from an older shrinkwrap file.

        Pruning the store makes no harm. It only makes installation a bit slower in case the unreferenced files will be needed again.
      `

    case 'root':
      return stripIndent`
        pnpm root [-g [--independent-leaves]]

        Options:

          -g                             print the global \`node_modules\` folder
          --independent-leaves           print the global \`node_modules\` folder installed with --independent-leaves option

        Print the effective \`node_modules\` folder.
      `

    case 'outdated':
      return stripIndent`
        pnpm outdated [[<@scope>/]<pkg> ...]

        Check for outdated packages.
      `

    case 'rebuild':
      return stripIndent`
        pnpm rebuild [[<@scope>/]<pkg> ...]

        Aliases: rb

        Rebuild a package.

        Options:
          --pending  rebuild packages that were not build during installation.
                     Packages are not build when installing with the --ignore-scripts flag
      `

    case 'server':
      return stripIndent`
        pnpm server start

        Starts a service that does all interactions with the store.
        Other commands will delegate any store-related tasks to this service.

        Options:

          --background                    runs the server in the background
          --protocol <auto|tcp|ipc>       the communication protocol used by the server
          --port <number>                 the port number to use, when TCP is used for communication
          --store                         the location where all the packages are saved on the disk.
          --network-concurrency <number>  maximum number of concurrent network requests
          --[no-]verify-store-integrity   if false, doesn't check whether packages in the store were mutated
          --[no-]lock
          --ignore-stop-requests          disallows stopping the server using \`pnpm server stop\`
          --ignore-upload-requests        disallows creating new side effect cache during install

        pnpm server stop

        Stops the store server.

        pnpm server status

        Prints information about the running server.
      `

    case 'recursive':
      return stripIndent`
        pnpm recursive [command] [flags] [-- <package selector>...]
        pnpm multi [command] [flags] [-- <package selector>...]
        pnpm m [command] [flags] [-- <package selector>...]

        Concurrently performs some actions in all subdirectories with a \`package.json\` (excluding node_modules).
        A \`pnpm-workspace.yaml\` file may be used to control what directories are searched for packages.

        Commands:

          install

          update

          uninstall [<@scope>/]<pkg>...
            Uninstall a dependency from each package

          link
            Deprecated. Use the install command with the link-workspace-packages config.
            runs installation in each package. If a package is available locally, the local version is linked.

          unlink
            Removes links to local packages and reinstalls them from the registry.

          list [[<@scope>/]<pkg>...]
            List dependencies in each package.

          outdated [[<@scope>/]<pkg>...]
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

          --shared-workspace-shrinkwrap
            Creates a single shrinkwrap.yaml file in the root of the workspace.
            A shared shrinkwrap also means that all dependencies of all workspace packages will be in a single node_modules.
      `

    default:
      return stripIndent`
        Usage: pnpm [command] [flags]

        Commands:

          - install
          - update
          - uninstall
          - link
          - unlink
          - list
          - outdated
          - prune
          - install-test
          - store add
          - store status
          - store prune
          - root
          - rebuild
          - import

          - recursive link
          - recursive unlink
          - recursive install
          - recursive update
          - recursive uninstall
          - recursive list
          - recursive outdated
          - recursive run
          - recursive test
          - recursive rebuild
          - recursive exec

          - server start
          - server stop
          - server status

        Other commands are passed through to npm
      `
  }
}
