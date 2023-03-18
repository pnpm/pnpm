import path from 'path'
import { runNpm } from '@pnpm/run-npm'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export async function configSet (opts: ConfigCommandOptions, key: string, value: string | null) {
  const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const _runNpm = runNpm.bind(null, opts.npmPath)
    if (value == null) {
      _runNpm(['config', 'delete', key])
    } else {
      _runNpm(['config', 'set', `${key}=${value}`])
    }
    return
  }
  const settings = await safeReadIniFile(configPath)
  if (value == null) {
    if (settings[key] == null) return
    delete settings[key]
  } else {
    settings[key] = value
  }
  await writeIniFile(configPath, settings)
}

function settingShouldFallBackToNpm (key: string): boolean {
  return (
    ['registry', '_auth', '_authToken', 'username', '_password'].includes(key) ||
    key[0] === '@' ||
    key.startsWith('//')
  )
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (err.code === 'ENOENT') return {}
    throw err
  }
}
