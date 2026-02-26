import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import * as dlx from './dlx.js'

export const commandNames = ['create']

export async function handler (_opts: dlx.DlxCommandOptions, params: string[]): Promise<{ exitCode: number } | string> {
  // If the first argument is --help or -h, we show the help message.
  if (params[0] === '--help' || params[0] === '-h') {
    return help()
  }

  const [packageName, ...packageArgs] = params
  if (packageName === undefined) {
    throw new PnpmError(
      'MISSING_ARGS',
      'Missing the template package name.\n' +
      'The correct usage is `pnpm create <name>` ' +
      'with <name> substituted for a package name.'
    )
  }

  const createPackageName = convertToCreateName(packageName)
  return dlx.handler(_opts, [createPackageName, ...packageArgs])
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    'allow-build': [String, Array],
  }
}

export function help (): string {
  return renderHelp({
    description: 'Creates a project from a `create-*` starter kit.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'A list of package names that are allowed to run postinstall scripts during installation',
            name: '--allow-build',
          },
        ],
      },
    ],
    url: docsUrl('create'),
    usages: [
      'pnpm create <name>',
      'pnpm create <name-without-create>',
      'pnpm create <@scope>',
    ],
  })
}

const CREATE_PREFIX = 'create-'

/**
 * Defines the npm's algorithm for resolving a package name
 * for create-* packages.
 *
 * Example:
 *   - `foo`            -> `create-foo`
 *   - `@usr/foo`       -> `@usr/create-foo`
 *   - `@usr`           -> `@usr/create`
 *   - `@usr@2.0.0`     -> `@usr/create@2.0.0`
 *   - `@usr/foo@2.0.0` -> `@usr/create-foo@2.0.0`
 *   - `@usr@latest`    -> `@user/create@latest`
 *
 * For more info, see https://docs.npmjs.com/cli/v9/commands/npm-init#description
 */
function convertToCreateName (packageName: string): string {
  if (packageName[0] === '@') {
    const preferredVersionPosition = packageName.indexOf('@', 1)
    let preferredVersion = ''
    if (preferredVersionPosition > -1) {
      preferredVersion = packageName.substring(preferredVersionPosition)
      packageName = packageName.substring(0, preferredVersionPosition)
    }
    const [scope, scopedPackage = ''] = packageName.split('/')

    if (scopedPackage === '') {
      return `${scope}/create${preferredVersion}`
    } else {
      return `${scope}/${ensureCreatePrefixed(scopedPackage)}${preferredVersion}`
    }
  } else {
    return ensureCreatePrefixed(packageName)
  }
}

function ensureCreatePrefixed (packageName: string): string {
  if (packageName.startsWith(CREATE_PREFIX)) {
    return packageName
  } else {
    return `${CREATE_PREFIX}${packageName}`
  }
}
