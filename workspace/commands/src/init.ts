import fs from 'node:fs'
import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import type { ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { getInitConfig } from './utils.js'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'init-package-manager',
      'init-type',
    ], allTypes),
    bare: Boolean,
  }
}

export const commandNames = ['init']

export function help (): string {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Set the module system for the package. Defaults to "commonjs".',
            name: '--init-type <commonjs|module>',
          },
          {
            description: 'Declare a pnpm version range via "devEngines.packageManager" in package.json and auto-download pnpm when it is missing',
            name: '--init-package-manager',
          },
          {
            description: 'Create a package.json file with the bare minimum of required fields',
            name: '--bare',
          },
        ],
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export type InitOptions =
  & Pick<ConfigContext, 'cliOptions'>
  & Partial<Pick<Config,
  | 'initPackageManager'
  | 'initType'
  >> & {
    bare?: boolean
    initAuthorName?: string
    initAuthorEmail?: string
    initAuthorUrl?: string
    initLicense?: string
    initVersion?: string
  }

export async function handler (opts: InitOptions, params?: string[]): Promise<string> {
  if (params?.length) {
    throw new PnpmError('INIT_ARG', 'init command does not accept any arguments', {
      hint: `Maybe you wanted to run "pnpm create ${params.join(' ')}"`,
    })
  }
  // Using cwd instead of the dir option because the dir option
  // is set to the first parent directory that has a package.json file
  // But --dir option from cliOptions should be respected.
  const manifestPath = path.join(opts.cliOptions.dir ?? process.cwd(), 'package.json')
  if (fs.existsSync(manifestPath)) {
    throw new PnpmError('PACKAGE_JSON_EXISTS', 'package.json already exists')
  }
  const manifest: ProjectManifest = opts.bare
    ? {}
    : {
      name: path.basename(process.cwd()),
      version: '1.0.0',
      description: '',
      main: 'index.js',
      scripts: {
        test: 'echo "Error: no test specified" && exit 1',
      },
      keywords: [],
      author: '',
      license: 'ISC',
    }

  if (opts.initType === 'module') {
    manifest.type = opts.initType
  }

  const initConfig = getInitConfig(opts)
  const packageJson = { ...manifest, ...initConfig }
  if (opts.initPackageManager) {
    packageJson.devEngines = {
      ...packageJson.devEngines,
      packageManager: {
        name: 'pnpm',
        version: `^${packageManager.version}`,
        onFail: 'download',
      },
    }
  }
  const priority = Object.fromEntries([
    'name',
    'version',
    'private',
    'description',
    'main',
    'scripts',
    'keywords',
    'author',
    'license',
    'devEngines',
  ].map((key, index) => [key, index]))
  const sortedPackageJson = sortKeysByPriority({ priority }, packageJson)
  await writeProjectManifest(manifestPath, sortedPackageJson, {
    indent: 2,
  })
  return `Wrote to ${manifestPath}

${JSON.stringify(sortedPackageJson, null, 2)}`
}
