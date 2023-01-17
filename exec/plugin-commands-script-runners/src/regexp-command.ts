export function isRegExpCommand (
  command: string
) {
  const flags = ['d', 'g', 'i', 'm', 's', 'u', 'y']
  return (new RegExp(`^/.+/(${flags.join('|')})*$`)).test(command)
}

export function buildRegExpFromCommand (
  command: string
): RegExp | null {
  if (!isRegExpCommand(command)) {
    return null
  }

  return new RegExp(command.slice(0, command.lastIndexOf('/')).slice(1))
}