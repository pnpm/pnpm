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
    description: 'The directory with links to the store (default is node_modules/.pnpm). All direct and indirect dependencies of the project are linked into this directory',
    name: '--virtual-store-dir <dir>',
  },
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
  {
    description: 'Run the command on the root workspace project',
    name: '--workspace-root',
    shortAlias: '-w',
  },
  {
    description: 'What level of logs to report. Any logs at or higher than the given level will be shown. Levels (lowest to highest): debug, info, warn, error. Or use "--silent" to turn off all logging.',
    name: '--loglevel <level>',
  },
  {
    description: 'Stream output from child processes immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.',
    name: '--stream',
  },
  {
    description: 'Aggregate output from child processes that are run in parallel, and only print output when child process is finished. It makes reading large logs after running `pnpm recursive` with `--parallel` or with `--workspace-concurrency` much easier (especially on CI). Only `--reporter=append-only` is supported.',
    name: '--aggregate-output',
  },
  {
    description: 'Divert all output to stderr',
    name: '--use-stderr',
  },
]
export const FILTERING = {
  list: [
    {
      description: 'Restricts the scope to package names matching the given pattern. E.g.: foo, "@bar/*"',
      name: '--filter <pattern>',
    },
    {
      description: 'Includes all direct and indirect dependencies of the matched packages. E.g.: foo...',
      name: '--filter <pattern>...',
    },
    {
      description: 'Includes only the direct and indirect dependencies of the matched packages without including the matched packages themselves. ^ must be doubled at the Windows Command Prompt. E.g.: foo^... (foo^^... in Command Prompt)',
      name: '--filter <pattern>^...',
    },
    {
      description: 'Includes all direct and indirect dependents of the matched packages. E.g.: ...foo, "...@bar/*"',
      name: '--filter ...<pattern>',
    },
    {
      description: 'Includes only the direct and indirect dependents of the matched packages without including the matched packages themselves. ^ must be doubled at the Windows Command Prompt. E.g.: ...^foo (...^^foo in Command Prompt)',
      name: '--filter ...^<pattern>',
    },
    {
      description: 'Includes all packages that are inside a given subdirectory. E.g.: ./components',
      name: '--filter ./<dir>',
    },
    {
      description: 'Includes all packages that are under the current working directory',
      name: '--filter .',
    },
    {
      description: 'Includes all projects that are under the specified directory. It may be used with "..." to select dependents/dependencies as well. It also may be combined with "[<since>]". For instance, all changed projects inside a directory: "{packages}[origin/master]"',
      name: '--filter {<dir>}',
    },
    {
      description: 'Includes all packages changed since the specified commit/branch. E.g.: "[master]", "[HEAD~2]". It may be used together with "...". So, for instance, "...[HEAD~1]" selects all packages changed in the last commit and their dependents',
      name: '--filter "[<since>]"',
    },
    {
      description: 'If a selector starts with ! (or \\! in zsh), it means the packages matching the selector must be excluded. E.g., "pnpm --filter !foo" selects all packages except "foo"',
      name: '--filter !<selector>',
    },
    {
      description: 'Defines files related to tests. Useful with the changed since filter. When selecting only changed packages and their dependent packages, the dependent packages will be ignored in case a package has changes only in tests. Usage example: pnpm --filter="...[origin/master]" --test-pattern="test/*" test',
      name: '--test-pattern <pattern>',
    },
    {
      description: 'Defines files to ignore when filtering for changed projects since the specified commit/branch. Usage example: pnpm --filter="...[origin/master]" --changed-files-ignore-pattern="**/README.md" build',
      name: '--changed-files-ignore-pattern <pattern>',
    },
    {
      description: 'Restricts the scope to package names matching the given pattern similar to --filter, but it ignores devDependencies when searching for dependencies and dependents.',
      name: '--filter-prod <pattern>',
    },
  ],
  title: 'Filtering options (run the command only on packages that satisfy at least one of the selectors)',
}

export const OUTPUT_OPTIONS = {
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
}
