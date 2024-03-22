import path from 'node:path'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'

import { runNpm } from '@pnpm/run-npm'
import type { ConfigCommandOptions } from '@pnpm/types'

export async function configSet(
  opts: ConfigCommandOptions,
  key: string | undefined,
  value: string | null
): Promise<void> {
  const configPath = opts.global
    ? path.join(opts.configDir, 'rc')
    : path.join(opts.dir, '.npmrc')

  if (opts.global && settingShouldFallBackToNpm(key ?? '')) {
    const _runNpm = runNpm.bind(null, opts.npmPath)

    if (value == null) {
      _runNpm(['config', 'delete', key ?? ''])
    } else {
      _runNpm(['config', 'set', `${key ?? ''}=${value}`])
    }

    return
  }

  const settings = await safeReadIniFile(configPath)

  if (value == null) {
    if (settings[key ?? ''] == null) {
      return
    }

    delete settings[key ?? '']
  } else {
    settings[key ?? ''] = value
  }

  await writeIniFile(configPath, settings)
}

function settingShouldFallBackToNpm(key: string): boolean {
  return (
    ['registry', '_auth', '_authToken', 'username', '_password'].includes(
      key
    ) ||
    key.startsWith('@') ||
    key.startsWith('//')
  )
}

async function safeReadIniFile(
  configPath: string
): Promise<Record<string, unknown>> {
  try {
    return (await readIniFile(configPath)) as Record<string, unknown>
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code === 'ENOENT') return {}
    throw err
  }
}
