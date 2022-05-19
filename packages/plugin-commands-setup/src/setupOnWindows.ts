import PnpmError from '@pnpm/error'
import { win32 as path } from 'path'
import execa from 'execa'
import { BadEnvVariableError } from './errors'

type IEnvironmentValueMatch = { groups: { name: string, type: string, data: string } } & RegExpMatchArray

const REG_KEY = 'HKEY_CURRENT_USER\\Environment'

export async function setupWindowsEnvironmentPath (pnpmHomeDir: string, opts: { force: boolean }): Promise<string> {
  // Use `chcp` to make `reg` use utf8 encoding for output.
  // Otherwise, the non-ascii characters in the environment variables will become garbled characters.
  const chcpResult = await execa('chcp')
  const cpMatch = /\d+/.exec(chcpResult.stdout) ?? []
  const cpBak = parseInt(cpMatch[0])
  if (chcpResult.failed || !(cpBak > 0)) {
    return `exec chcp failed: ${cpBak}, ${chcpResult.stderr}`
  }
  await execa('chcp', ['65001'])
  try {
    return await _setupWindowsEnvironmentPath(path.normalize(pnpmHomeDir), opts)
  } finally {
    await execa('chcp', [cpBak.toString()])
  }
}

async function _setupWindowsEnvironmentPath (pnpmHomeDir: string, opts: { force: boolean }): Promise<string> {
  const registryOutput = await getRegistryOutput()
  const logger: string[] = []
  logger.push(logEnvUpdate(await updateEnvVariable(registryOutput, 'PNPM_HOME', pnpmHomeDir, opts), 'PNPM_HOME'))
  logger.push(logEnvUpdate(await prependToPath(registryOutput, '%PNPM_HOME%'), 'Path'))

  return logger.join('\n')
}

function logEnvUpdate (envUpdateResult: 'skipped' | 'updated', envName: string): string {
  switch (envUpdateResult) {
  case 'skipped': return `${envName} was already up-to-date`
  case 'updated': return `${envName} was updated`
  }
  return ''
}

async function updateEnvVariable (registryOutput: string, name: string, value: string, opts: { force: boolean }) {
  const currentValue = await getEnvValueFromRegistry(registryOutput, name)
  if (currentValue && !opts.force) {
    if (currentValue !== value) {
      throw new BadEnvVariableError({ envName: name, currentValue, wantedValue: value })
    }
    return 'skipped'
  } else {
    await setEnvVarInRegistry(name, value)
    return 'updated'
  }
}

async function prependToPath (registryOutput: string, prependDir: string) {
  const pathData = await getEnvValueFromRegistry(registryOutput, 'Path')
  if (pathData === undefined || pathData == null || pathData.trim() === '') {
    throw new PnpmError('NO_PATH', '"Path" environment variable is not found in the registry')
  } else if (pathData.split(path.delimiter).includes(prependDir)) {
    return 'skipped'
  } else {
    const newPathValue = `${prependDir}${path.delimiter}${pathData}`
    await setEnvVarInRegistry('Path', newPathValue)
    return 'updated'
  }
}

// `windowsHide` in `execa` is true by default, which will cause `chcp` to have no effect.
const EXEC_OPTS = { windowsHide: false }

/**
 * We read all the registry values and then pick the keys that we need.
 * This is done because if we would try to pick a key that is not in the registry, the command would fail.
 * And it is hard to identify the real cause of the command failure.
 */
async function getRegistryOutput (): Promise<string> {
  try {
    const queryResult = await execa('reg', ['query', REG_KEY], EXEC_OPTS)
    return queryResult.stdout
  } catch (err: any) { // eslint-disable-line
    throw new PnpmError('REG_READ', 'win32 registry environment values could not be retrieved')
  }
}

async function getEnvValueFromRegistry (registryOutput: string, envVarName: string): Promise<string | undefined> {
  const regexp = new RegExp(`^ {4}(?<name>${envVarName}) {4}(?<type>\\w+) {4}(?<data>.*)$`, 'gim')
  const match = Array.from(registryOutput.matchAll(regexp))[0] as IEnvironmentValueMatch
  return match?.groups.data
}

async function setEnvVarInRegistry (envVarName: string, envVarValue: string) {
  try {
    await execa('reg', ['add', REG_KEY, '/v', envVarName, '/t', 'REG_EXPAND_SZ', '/d', envVarValue, '/f'], EXEC_OPTS)
  } catch (err: any) { // eslint-disable-line
    throw new PnpmError('FAILED_SET_ENV', `Failed to set "${envVarName}" to "${envVarValue}": ${err.stderr as string}`)
  }
  // When setting environment variables through the registry, they will not be recognized immediately.
  // There is a workaround though, to set at least one environment variable with `setx`.
  // We have some redundancy here because we run it for each env var.
  // It would be enough also to run it only for the last changed env var.
  // Read more at: https://bit.ly/39OlQnF
  await execa('setx', [envVarName, envVarValue], EXEC_OPTS)
}
