import { URL } from 'url'
import { types as allTypes } from '@pnpm/config'
import { docsUrl } from '@pnpm/cli-utils'
import renderHelp from 'render-help'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'

export type PkgManagerFieldSpec = `${
  'npm' | 'pnpm' | 'yarn'
}@${number}.${number}.${number}${
  '' | `-${string}`
}`
export const pkgManagerFieldValid = (s: string): s is PkgManagerFieldSpec =>
  /^npm|pnpm|yarn@[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9-]+)?$/.test(s)

export type OptionsRaw = Readonly<{
  loglevel?: 'silent' | 'error' | 'warn' | 'info' | 'debug'
  scope?: string
  force?: true
  'workspace-update'?: string
  'init-type'?: 'module' | 'commonjs'
  'init-package-manager'?: PkgManagerFieldSpec
  'init-private'?: true
  'init-ask'?: true | 'extended' | 'npm' | 'none'
  'init-contributors'?: string | string[]
  'init-funding'?: string | string[]
  'init-name'?: string
  'init-homepage'?: string
  'init-bugs-url'?: string
  'init-bugs-email'?: string
  'init-author'?: string
  'init-publish-config'?: string
  'init-author-email'?: string
  'init-author-name'?: string
  'init-author-url'?: string
  'init-license'?: string
  'init-module'?: string
  'init-version'?: string
  'init-description'?: string
  'init-main'?: string
  'init-script-test'?: string
  'init-repository'?: string | { type: string, url: string }
  'init-keywords'?: string[]
}>

const getOptionTypes = () => ({
  ...pick([
    'scope',
    'force',
    'loglevel',
    'init-author-email',
    'init-author-name',
    'init-author-url',
    'init-license',
    'init-version',
    'init-module',
  ], allTypes),
  'init-type': ['module', 'commonjs'],
  'init-package-manager': String,
  'init-private': [null, true],
  'init-ask': [null, true, 'extended', 'npm', 'none'],
  'init-contributors': [String, Array],
  'init-funding': [URL, Array],
  'init-name': String,
  'init-homepage': [String, URL],
  'init-bugs-url': [String, URL],
  'init-bugs-email': String,
  'init-author': String,
  'init-publish-config': String,
  'workspace-update': Boolean,
  'init-author-email': String,
  'init-author-name': String,
  'init-author-url': String,
  'init-license': String,
  'init-module': String,
  'init-version': String,
  'init-description': String,
  'init-main': String,
  'init-script-test': String,
  'init-repository': String,
  'init-keywords': [String, Array],
} satisfies Record<keyof OptionsRaw, unknown>)

export function cliOptionsTypes (): Record<string, unknown> {
  return getOptionTypes()
}
export function rcOptionsTypes (): Record<string, unknown> {
  return omit([
    'force',
    'loglevel',
    'init-name',
  ], getOptionTypes())
}

export const optionsInfo = {
  '--scope': 'Prepend the scope to package name in package.json',
  '--force': 'Create a package.json file even if one already exists',
  '--loglevel': 'Set to silent to suppress output entirely, to warn to show only warnings and errors, or to info or debug to show all logs.',
  '--workspace-update': 'Add this package to a workspace root configuration if one exists',
  '--init-ask': 'Set to true or "npm" to ask only the the same questions as npm, or "extended" to ask all questions relevant via cli.',
  '--init-author-email': 'The value that should be used by default for the package author\'s email.',
  '--init-author-name': 'The value that should be used by default for the package author\'s name.',
  '--init-author-url': 'The value that should be used by default for the package author\'s website/homepage.',
  '--init-license': 'The value that should be used by default for the package license.',
  '--init-module': 'A module that will be loaded by the pnpm init command. See the documentation for the init-package-json module for more information',
  '--init-version': 'The value that should be used by default for the package version number, if not already set in package.json',
  '--init-homepage': 'The URL to the project homepage.',
  '--init-bugs-url': 'The URL to your project\'s issue tracker or where issues should be submitted.',
  '--init-bugs-email': 'The email address to which issues should be reported (the value of the bugs field in package.json will be made an object instead of a string to accommodate).',
  '--init-author': 'In place of individual author fields, you can also set the author field to a string of the form "Name <email> (url)".',
  '--init-contributors': 'Provide as many contributors as you like with the format "Name <email> (url)".',
  '--init-funding': 'Provide as many funding URLs as you like.',
  '--init-private': 'Set the package to private, making it not publishable to the registry.',
  '--init-publish-config': 'Provide stringified JSON to be used as the publishConfig field in the package.json file.',
  '--init-type': 'Set the type of the package to be initialized. Can be either "module" or "commonjs".',
  '--init-package-manager': 'The package manager to be used for the project. Can be either "npm", "pnpm" or "yarn", followed by an "@" sign and the exact version number.',
  '--init-name': 'Set the name of the package.',
  '--init-description': 'Set the description of the package.',
  '--init-main': 'Set the entry point of the package.',
  '--init-script-test': 'Set the test command of the package.',
  '--init-repository': 'Set the repository field of the package.json file.',
  '--init-keywords': 'Set the keywords of the package. Provide as many keywords as you like.',
} satisfies {
  [key in `--${keyof OptionsRaw}`]: string
}
export function help (): string {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [
      {
        title: 'Options',
        list: Object.entries(optionsInfo).map(([name, description]) => ({ name, description })),
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init [options]'],
  })
}
