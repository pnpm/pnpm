import renderHelp from 'render-help'
import PnpmError from '@pnpm/error'
import * as dlx from './dlx'

export const commandNames = ['create']

export async function handler (_opts: Record<string, never>, params: string[]) {
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
  return dlx.handler({}, [createPackageName, ...packageArgs])
}

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description: 'Creates a project from a `create-*` starter kit.',
    // TODO: add when the docs page is available
    // url: docsUrl('create'),
    usages: [
      'pnpm create <name>',
      'pnpm create <name-without-create>',
      'pnpm create <@scope>',
    ],
  })
}

const createPrefix = 'create-'

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
    } else if (scopedPackage.startsWith(createPrefix)) {
      return packageName
    } else {
      return `${scope}/${createPrefix}${scopedPackage}`
    }
  } else {
    if (packageName.startsWith(createPrefix)) {
      return packageName
    } else {
      return `${createPrefix}${packageName}`
    }
  }
}
