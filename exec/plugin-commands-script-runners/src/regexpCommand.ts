export function tryBuildRegExpFromCommand (
  command: string
): RegExp | null {
  if (command.length < 3) {
    return null
  }
  if (command[0] !== '/' || command.lastIndexOf('/') < 1) {
    return null
  }
  try {
    return new RegExp(command.slice(0, command.lastIndexOf('/')).slice(1))
  } catch {
    return null
  }
}
