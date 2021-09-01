import execa from 'execa'

type IEnvironmentValueMatch = { groups: { name: string, type: string, data: string } } & RegExpMatchArray

export async function setupEnvironmentPath (pnpmHomeDir: string): Promise<string> {
  const pathRegex = /^ {4}(?<name>PATH) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const pnpmHomeRegex = /^ {4}(?<name>PNPM_HOME) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const regKey = 'HKEY_CURRENT_USER\\Environment'

  const queryResult = await execa('reg', ['query', regKey])

  if (queryResult.failed) {
    return 'Win32 registry environment values could not be retrieved'
  }

  const queryOutput = queryResult.stdout
  const pathValueMatch = [...queryOutput.matchAll(pathRegex)] as IEnvironmentValueMatch[]
  const homeValueMatch = [...queryOutput.matchAll(pnpmHomeRegex)] as IEnvironmentValueMatch[]

  const logger = []
  if (homeValueMatch.length === 1) {
    logger.push(`Currently 'PNPM_HOME' is set to '${homeValueMatch[0].groups.data}'`)
  } else {
    logger.push(`Setting 'PNPM_HOME' to value '${pnpmHomeDir}'`)
    const addResult = await execa('reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', pnpmHomeDir, '/f'])
    if (addResult.failed) {
      logger.push(`\t${addResult.stderr}`)
    } else {
      logger.push(`\t${addResult.stdout}`)
    }
  }

  if (pathValueMatch.length === 1) {
    const pathData = pathValueMatch[0].groups.data
    if (pathData == null || pathData.trim() === '') {
      logger.push('Current PATH is empty. No changes to this environment variable are applied')
    } else {
      const pathDataUpperCase = pathData.toUpperCase()
      if (pathDataUpperCase.includes('%PNPM_HOME%')) {
        logger.push('PATH already contains PNPM_HOME')
      } else {
        logger.push('Updating PATH')
        const addResult = await execa('reg', ['add', regKey, '/v', pathValueMatch[0].groups.name, '/t', 'REG_EXPAND_SZ', '/d', `${pathData}%PNPM_HOME%;`, '/f'])
        if (addResult.failed) {
          logger.push(`\t${addResult.stderr}`)
        } else {
          logger.push(`\t${addResult.stdout}`)
        }
      }
    }
  } else {
    logger.push('Current PATH is not set. No changes to this environment variable are applied')
  }

  return logger.join('\n')
}
