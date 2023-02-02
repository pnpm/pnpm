import { PnpmError } from '@pnpm/error'

export function tryBuildRegExpFromCommand (command: string): RegExp | null {
  // https://github.com/stdlib-js/regexp-regexp/blob/6428051ac9ef7c9d03468b19bdbb1dc6fc2a5509/lib/regexp.js
  const regExpDetectRegExpScriptCommand = /^\/((?:\\\/|[^/])+)\/([dgimuys]*)$/
  const match = command.match(regExpDetectRegExpScriptCommand)

  // if the passed script selector is not in the format of RegExp literal like /build:.*/, return null and handle it as a string script command
  if (!match) {
    return null
  }

  // if the passed RegExp script selector includes flag, report the error because RegExp flag is not useful for script selector and pnpm does not support this.
  if (match[2]) {
    throw new PnpmError('UNSUPPORTED_SCRIPT_COMMAND_FORMAT', 'RegExp flags are not supported in script command selector')
  }

  try {
    return new RegExp(match[1])
  } catch {
    return null
  }
}
