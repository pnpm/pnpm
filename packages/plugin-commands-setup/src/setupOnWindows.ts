import PnpmError from '@pnpm/error'
import { win32 as path } from 'path'
import execa from 'execa'

type IEnvironmentValueMatch = { groups: { name: string, type: string, data: string } } & RegExpMatchArray

const REG_KEY = 'HKEY_CURRENT_USER\\Environment'

function findEnvValuesInRegistry (regEntries: string, envVarName: string): IEnvironmentValueMatch[] {
  const regexp = new RegExp(`^ {4}(?<name>${envVarName}) {4}(?<type>\\w+) {4}(?<data>.*)$`, 'gim')
  return Array.from(regEntries.matchAll(regexp)) as IEnvironmentValueMatch[]
}

function setEnvVarInRegistry (envVarName: string, envVarValue: string) {
  return execa('reg', ['add', REG_KEY, '/v', envVarName, '/t', 'REG_EXPAND_SZ', '/d', envVarValue, '/f'])
}

function pathIncludesDir (pathValue: string, dir: string): boolean {
  const dirPath = path.parse(path.normalize(dir))
  return pathValue
    .split(path.delimiter)
    .map(p => path.normalize(p))
    .map(p => path.parse(p))
    .map(p => `${p.dir}${path.sep}${p.base}`.toUpperCase())
    .filter(p => p !== '')
    .includes(`${dirPath.dir}${path.sep}${dirPath.base}`.toUpperCase())
}

export async function setupWindowsEnvironmentPath (pnpmHomeDir: string, opts: { force: boolean }): Promise<string> {
  // Use `chcp` to make `reg` use utf8 encoding for output.
  // Otherwise, the non-ascii characters in the environment variables will become garbled characters.
  const queryResult = await execa(`chcp 65001>nul && reg query ${REG_KEY}`, undefined, { shell: true })

  if (queryResult.failed) {
    return 'Win32 registry environment values could not be retrieved'
  }

  const queryOutput = queryResult.stdout
  const pathValueMatch = findEnvValuesInRegistry(queryOutput, 'PATH')
  const homeValueMatch = findEnvValuesInRegistry(queryOutput, 'PNPM_HOME')

  let commitNeeded = false
  const logger = []

  if (homeValueMatch.length === 1 && !opts.force) {
    const currentHomeDir = homeValueMatch[0].groups.data
    if (currentHomeDir !== pnpmHomeDir) {
      throw new PnpmError('DIFFERENT_HOME_DIR_IS_SET', `Currently 'PNPM_HOME' is set to '${currentHomeDir}'`, {
        hint: 'If you want to override the existing PNPM_HOME env variable, use the --force option',
      })
    }
  } else {
    logger.push(`Setting 'PNPM_HOME' to value '${pnpmHomeDir}'`)
    const addResult = await setEnvVarInRegistry('PNPM_HOME', pnpmHomeDir)
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
      if (pathIncludesDir(pathData, pnpmHomeDir)) {
        logger.push('PATH already contains PNPM_HOME')
      } else {
        logger.push('Updating PATH')
        const newPathValue = `${pnpmHomeDir}${path.delimiter}${pathData}`
        const addResult = await setEnvVarInRegistry(pathValueMatch[0].groups.name, newPathValue)
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
    await execa('setx', ['PNPM_HOME', pnpmHomeDir])
  }

  return logger.join('\n')
}
