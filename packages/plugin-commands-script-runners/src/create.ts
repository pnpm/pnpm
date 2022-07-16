import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { types } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import pick from 'ramda/src/pick.js'
import * as dlx from './dlx'

export const commandNames = ['create']

export async function handler (_opts: dlx.DlxCommandOptions, params: string[]) {
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

export function rcOptionsTypes () {
  return {
    ...pick([
      'use-node-version',
    ], types),
  }
}

export function cliOptionsTypes () {
  return {
    ...rcOptionsTypes(),
  }
}

export function help () {
  return renderHelp({
    description: 'Creates a project from a `create-*` starter kit.',
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
 *   - `foo`      -> `create-foo`
 *   - `@usr/foo` -> `@usr/create-foo`
 *   - `@usr`     -> `@usr/create`
 *
 * For more info, see https://docs.npmjs.com/cli/v7/commands/npm-init#description
 */
function convertToCreateName (packageName: string) {
  if (packageName.startsWith('@')) {
    const [scope, scopedPackage = ''] = packageName.split('/')

    if (scopedPackage === '') {
      return `${scope}/create`
    } else {
      return `${scope}/${ensureCreatePrefixed(scopedPackage)}`
    }
  } else {
    return ensureCreatePrefixed(packageName)
  }
}

function ensureCreatePrefixed (packageName: string) {
  if (packageName.startsWith(CREATE_PREFIX)) {
    return packageName
  } else {
    return `${CREATE_PREFIX}${packageName}`
  }
}
