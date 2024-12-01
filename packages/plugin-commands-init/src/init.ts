import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type CliOptions, type UniversalOptions } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import renderHelp from 'render-help'
import { prompt } from 'enquirer'
import { parseRawConfig } from './utils'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    yes: Boolean,
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
            description: 'Skip all the questions and use the defaults',
            name: '--yes',
            shortAlias: '-y',
          },
        ],
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'rawConfig'> & { cliOptions: CliOptions & { yes?: boolean } },
  params?: string[]
): Promise<string> {
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

  let manifest = {
    name: path.basename(process.cwd()),
    version: '1.0.0',
    description: '',
    main: 'index.js',
    repository: {
      type: 'git',
      url: '',
    },
    scripts: { test: 'echo "Error: no test specified" && exit 1' },
    keywords: [],
    author: '',
    license: 'ISC',
  }
  if (!opts.cliOptions.yes) {
    const { testCommand, repositoryUrl, ...answeredManifest } = await prompt<{
      name: string
      version: string
      description: string
      main: string
      testCommand: string
      repositoryUrl: string
      keywords: string[]
      author: string
      license: string
    }>([
      {
        type: 'input',
        name: 'name',
        message: 'package name',
        initial: manifest.name,
        validate: (value) =>
          /^[a-zA-Z0-9-_]+$/.test(value) ? true : 'Only letters, numbers, dashes, and underscores are allowed',
      },
      {
        type: 'input',
        name: 'version',
        message: 'version',
        initial: manifest.version,
        validate: (value) =>
          /^\d+\.\d+\.\d+$/.test(value) ? true : 'Version must follow semantic versioning (e.g., 1.0.0)',
      },
      {
        type: 'input',
        name: 'description',
        message: 'description',
        initial: manifest.description,
      },
      {
        type: 'input',
        name: 'main',
        message: 'entry point',
        initial: manifest.main,
      },
      {
        type: 'input',
        name: 'testCommand',
        message: 'test command',
        initial: '',
      },
      {
        type: 'input',
        name: 'repositoryUrl',
        message: 'git repository',
        initial: '',
      },
      {
        type: 'list',
        name: 'keywords',
        message: 'keywords',
        initial: '',
      },
      {
        type: 'input',
        name: 'author',
        message: 'author',
        initial: manifest.author,
      },
      {
        type: 'input',
        name: 'license',
        message: 'license',
        initial: manifest.license,
      },
    ])
    manifest = Object.assign(manifest, answeredManifest, {
      repository: {
        ...manifest.repository,
        url: repositoryUrl || manifest.repository.url,
      },
      scripts: {
        ...manifest.scripts,
        test: testCommand || manifest.scripts.test,
      },
    })
  }
  const config = await parseRawConfig(opts.rawConfig)
  const packageJson = { ...manifest, ...config }

  if (!opts.cliOptions.yes) {
    console.log(`About to write to ${manifestPath}:`)

    const answer = await prompt<{
      confirm: boolean
    }>([
      {
        type: 'confirm',
        name: 'confirm',
        message: 'Is this OK?',
        initial: true,
      },
    ])
    if (!answer.confirm) {
      return 'Aborted.'
    }
  }

  await writeProjectManifest(manifestPath, packageJson, {
    indent: 2,
  })
  return `Wrote to ${manifestPath}

${JSON.stringify(packageJson, null, 2)}`
}
