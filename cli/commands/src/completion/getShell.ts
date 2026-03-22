import { PnpmError } from '@pnpm/error'
import { isShellSupported, SUPPORTED_SHELLS, type SupportedShell } from '@pnpm/tabtab'

export function getShellFromString (shell?: string): SupportedShell {
  shell = shell?.trim()

  if (!shell) {
    throw new PnpmError('MISSING_SHELL_NAME', '`pnpm completion` requires a shell name')
  }

  if (!isShellSupported(shell)) {
    throw new PnpmError('UNSUPPORTED_SHELL', `'${shell}' is not supported`, {
      hint: `Supported shells are: ${SUPPORTED_SHELLS.join(', ')}`,
    })
  }

  return shell
}

export function getShellFromParams (params: string[]): SupportedShell {
  const [shell, ...rest] = params

  if (rest.length) {
    throw new PnpmError('REDUNDANT_PARAMETERS', `The ${rest.length} parameters after shell is not necessary`)
  }

  return getShellFromString(shell)
}
