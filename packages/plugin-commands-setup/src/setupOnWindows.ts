import { win32 as path } from 'path'
import execa from 'execa'

type IEnvironmentValueMatch = { groups: { name: string, type: string, data: string } } & RegExpMatchArray

export async function setupWindowsEnvironmentPath (pnpmHomeDir: string): Promise<string> {
  const pathRegex = /^ {4}(?<name>PATH) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const pnpmHomeRegex = /^ {4}(?<name>PNPM_HOME) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const regKey = 'HKEY_CURRENT_USER\\Environment'

  // Use `chcp` to make `reg` use utf8 encoding for output.
  // Otherwise, the non-ascii characters in the environment variables will become garbled characters.
  const queryResult = await execa(`chcp 65001>nul && reg query ${regKey}`, undefined, { shell: true })

  if (queryResult.failed) {
    return 'Win32 registry environment values could not be retrieved'
  }

  const queryOutput = queryResult.stdout
  const pathValueMatch = [...queryOutput.matchAll(pathRegex)] as IEnvironmentValueMatch[]
  const homeValueMatch = [...queryOutput.matchAll(pnpmHomeRegex)] as IEnvironmentValueMatch[]

  let commitNeeded = false
  let homeDir = pnpmHomeDir
  const logger = []

  if (homeValueMatch.length === 1) {
    homeDir = homeValueMatch[0].groups.data
    logger.push(`Currently 'PNPM_HOME' is set to '${homeDir}'`)
  } else {
    logger.push(`Setting 'PNPM_HOME' to value '${homeDir}'`)
    const addResult = await execa('reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', homeDir, '/f'])
    if (addResult.failed) {
      logger.push(`\t${addResult.stderr}`)
    } else {
      commitNeeded = true
      logger.push(`\t${addResult.stdout}`)
    }
  }

  if (pathValueMatch.length === 1) {
    const pathData = pathValueMatch[0].groups.data
    if (pathData == null || pathData.trim() === '') {
      logger.push('Current PATH is empty. No changes to this environment variable are applied')
    } else {
      const homeDirPath = path.parse(path.normalize(homeDir))

      if (pathData
        .split(path.delimiter)
        .map(p => path.normalize(p))
        .map(p => path.parse(p))
        .map(p => `${p.dir}${path.sep}${p.base}`.toUpperCase())
        .filter(p => p !== '')
        .includes(`${homeDirPath.dir}${path.sep}${homeDirPath.base}`.toUpperCase())) {
        logger.push('PATH already contains PNPM_HOME')
      } else {
        logger.push('Updating PATH')
        const addResult = await execa('reg', ['add', regKey, '/v', pathValueMatch[0].groups.name, '/t', 'REG_EXPAND_SZ', '/d', `${homeDir}${path.delimiter}${pathData}`, '/f'])
        if (addResult.failed) {
          logger.push(`\t${addResult.stderr}`)
        } else {
          commitNeeded = true
          logger.push(`\t${addResult.stdout}`)
        }
      }
    }
  } else {
    logger.push('Current PATH is not set. No changes to this environment variable are applied')
  }

  if (commitNeeded) {
    await execa('setx', ['PNPM_HOME', homeDir])
  }

  return logger.join('\n')
}
