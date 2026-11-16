import chalk from 'chalk'

export function formatUnknownOptionsError (
  unknownOptions: Map<string, string[]>,
  unknownOptionShorthands?: Map<string, string>
): string {
  let output = chalk.bgRed.red('[') + chalk.bgRed.black('ERROR') + chalk.bgRed.red(']')
  const unknownOptionsArray = Array.from(unknownOptions.keys())
  // An option passed via a shorthand is reported as typed: '-l', not 'long'
  const displayName = (unknownOption: string) => unknownOptionShorthands?.get(unknownOption) ?? unknownOption
  if (unknownOptionsArray.length > 1) {
    return `${output} ${chalk.red(`Unknown options: ${unknownOptionsArray.map(unknownOption => `'${displayName(unknownOption)}'`).join(', ')}`)}`
  }
  const unknownOption = unknownOptionsArray[0]
  output += ` ${chalk.red(`Unknown option: '${displayName(unknownOption)}'`)}`
  const didYouMeanOptions = unknownOptions.get(unknownOption)
  if (!didYouMeanOptions?.length) {
    return output
  }
  return `${output}
Did you mean '${didYouMeanOptions.join("', or '")}'? Use "--config.unknown=value" to force an unknown option.`
}
