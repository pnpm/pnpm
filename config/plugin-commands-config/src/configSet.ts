import fs from 'fs'
import path from 'path'
import util from 'util'
import { runNpm } from '@pnpm/run-npm'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import camelCase from 'camelcase'
import kebabCase from 'lodash.kebabcase'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export async function configSet (opts: ConfigCommandOptions, key: string, value: string | null): Promise<void> {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const _runNpm = runNpm.bind(null, opts.npmPath)
    if (value == null) {
      _runNpm(['config', 'delete', key])
    } else {
      _runNpm(['config', 'set', `${key}=${value}`])
    }
    return
  }
  if (opts.global === true || fs.existsSync(path.join(opts.dir, '.npmrc'))) {
    const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
    const settings = await safeReadIniFile(configPath)
    key = kebabCase(key)
    if (value == null) {
      if (settings[key] == null) return
      delete settings[key]
    } else {
      settings[key] = value
    }
    await writeIniFile(configPath, settings)
    return
  }
  key = camelCase(key)
  await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
    [key]: value,
  })
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
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
