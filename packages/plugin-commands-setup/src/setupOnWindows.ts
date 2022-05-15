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
  const logger: string[] = []
  logger.push(logEnvUpdate(await updateEnvVariable('PNPM_HOME', pnpmHomeDir, opts), 'PNPM_HOME'))
  logger.push(logEnvUpdate(await prependToPath('%PNPM_HOME%'), 'Path'))

  return logger.join('\n')
}

function logEnvUpdate (envUpdateResult: 'skipped' | 'updated', envName: string): string {
  switch (envUpdateResult) {
  case 'skipped': return `${envName} was already up-to-date`
  case 'updated': return `${envName} was updated`
  }
  return ''
}

async function updateEnvVariable (name: string, value: string, opts: { force: boolean }) {
  const currentValue = await getEnvValueFromRegistry(name)
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

async function prependToPath (prependDir: string) {
  const pathData = await getEnvValueFromRegistry('Path')
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

async function getEnvValueFromRegistry (envVarName: string): Promise<string | undefined> {
  const queryResult = await execa('reg', ['query', REG_KEY, '/v', envVarName], { windowsHide: false })
  if (queryResult.failed) {
    throw new PnpmError('REG_READ', 'Win32 registry environment values could not be retrieved')
  }
  const regexp = new RegExp(`^ {4}(?<name>${envVarName}) {4}(?<type>\\w+) {4}(?<data>.*)$`, 'gim')
  const match = Array.from(queryResult.stdout.matchAll(regexp))[0] as IEnvironmentValueMatch
  return match?.groups.data
}

async function setEnvVarInRegistry (envVarName: string, envVarValue: string) {
  // `windowsHide` in `execa` is true by default, which will cause `chcp` to have no effect.
  const addResult = await execa('reg', ['add', REG_KEY, '/v', envVarName, '/t', 'REG_EXPAND_SZ', '/d', envVarValue, '/f'], { windowsHide: false })
  if (addResult.failed) {
    throw new PnpmError('FAILED_SET_ENV', `Failed to set "${envVarName}" to "${envVarValue}": ${addResult.stderr}`)
  } else {
    // When setting environment variables through the registry, they will not be recognized immediately.
    // There is a workaround though, to set at least one environment variable with `setx`.
    // We have some redundancy here because we run it for each env var.
    // It would be enough also to run it only for the last changed env var.
    // Read more at: https://bit.ly/39OlQnF
    await execa('setx', [envVarName, envVarValue])
  }
}
