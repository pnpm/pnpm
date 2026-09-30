import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { type Config, type ConfigContext, getDefaultWorkspaceConcurrency, types as allTypes, type UniversalOptions } from '@pnpm/config.reader'
import chalk from 'chalk'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { api, toPackResultJson } from './packApi.js'
import { type PackDestinationLocker, packRecursively } from './packRecursive.js'

export { api, type PackResult, resolvePackOutput } from './packApi.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...cliOptionsTypes(),
    ...pick([
      'npm-path',
      'skip-manifest-obfuscation',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    out: String,
    recursive: Boolean,
    ...pick([
      'dry-run',
      'pack-destination',
      'pack-gzip-level',
      'json',
      'skip-manifest-obfuscation',
      'workspace-concurrency',
    ], allTypes),
  }
}

export const commandNames = ['pack']

export function help (): string {
  return renderHelp({
    description: 'Create a tarball from a package',
    usages: ['pnpm pack'],
    descriptionLists: [
      {
        title: 'Options',

        list: getPackOptionsHelp(),
      },
      FILTERING,
    ],
  })
}

function getPackOptionsHelp (): Array<{ description: string, name: string, shortAlias?: string }> {
  return [
    {
      description: 'Does everything `pnpm pack` would do except actually writing the tarball to disk.',
      name: '--dry-run',
    },
    {
      description: 'Directory in which `pnpm pack` will save tarballs. The default is the current working directory.',
      name: '--pack-destination <dir>',
    },
    {
      description: 'Prints the packed tarball and contents in the json format.',
      name: '--json',
    },
    {
      description: 'Customizes the output path for the tarball. Use `%s` and `%v` to include the package name and version, e.g., `%s.tgz` or `some-dir/%s-%v.tgz`. By default, the tarball is saved in the current working directory with the name `<package-name>-<version>.tgz`.',
      name: '--out <path>',
    },
    {
      description: 'Pack all packages from the workspace',
      name: '--recursive',
      shortAlias: '-r',
    },
    {
      description: 'Skip pnpm\'s manifest obfuscation: keep the original `packageManager` field and publish lifecycle scripts in the packed manifest instead of stripping them. The pnpm-specific `pnpm` field is still omitted.',
      name: '--skip-manifest-obfuscation',
    },
    {
      description: `Set the maximum number of concurrency. Default is ${getDefaultWorkspaceConcurrency()}. For unlimited concurrency use Infinity.`,
      name: '--workspace-concurrency <number>',
    },
  ]
}

export type PackOptions = Pick<UniversalOptions, 'dir'> & Pick<Config, 'catalogs'
| 'ignoreScripts'
| 'embedReadme'
| 'packGzipLevel'
| 'nodeLinker'
| 'skipManifestObfuscation'
| 'userAgent'
> & Partial<Pick<Config, 'extraBinPaths'
| 'extraEnv'
| 'recursive'
| 'workspaceConcurrency'
| 'workspaceDir'
// Registry-storage changelog composition (see `injectChangelog`): the
// registry to read the previous version's tarball from, plus the network
// config `createFetchFromRegistry` needs.
| 'versioning'
| 'registriesByScope'
| 'configByUri'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'ca'
| 'cert'
| 'key'
| 'strictSsl'
| 'httpProxy'
| 'httpsProxy'
| 'noProxy'
| 'localAddress'
>> & Partial<Pick<ConfigContext,
| 'hooks'
| 'allProjects'
| 'selectedProjectsGraph'
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
>> & {
  argv: {
    original: string[]
  }
  dryRun?: boolean
  engineStrict?: boolean
  packDestination?: string
  out?: string
  packDestinationLocker?: PackDestinationLocker
  json?: boolean
  unicode?: boolean
}

export interface PackResultJson {
  name: string
  version: string
  filename: string
  files: Array<{ path: string }>
}

export async function handler (opts: PackOptions): Promise<string> {
  const packedPackages: PackResultJson[] = opts.recursive
    ? await packRecursively(opts)
    : [toPackResultJson(await api(opts))]

  if (opts.json) {
    return JSON.stringify(packedPackages.length > 1 ? packedPackages : packedPackages[0], null, 2)
  }

  return packedPackages.map(
    ({ name, version, filename, files }) => `${opts.unicode ? '📦 ' : 'package:'} ${name}@${version}
${chalk.blueBright('Tarball Contents')}
${files.map(({ path }) => path).join('\n')}
${chalk.blueBright('Tarball Details')}
${filename}`
  ).join('\n\n')
}
