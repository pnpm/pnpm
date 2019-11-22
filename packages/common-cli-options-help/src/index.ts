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
  }
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
]
export const FILTERING = {
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
      name: '--filter ./<dir>',
    },
    {
      description: 'Includes all packages that are under the current working directory',
      name: '--filter .',
    },
  ],
  title: 'Filtering options (run the command only on packages that satisfy at least one of the selectors)',
}
