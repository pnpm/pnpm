import fs from 'node:fs'
import path from 'node:path'

import { readIniFileSync } from 'read-ini-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeIniFileSync } from 'write-ini-file'
import { writeYamlFileSync } from 'write-yaml-file'

import type { config } from '../../src/index.js'

export function getOutputString (result: config.ConfigHandlerResult): string {
  if (result == null) throw new Error('output is null or undefined')
  if (typeof result === 'string') return result
  if (typeof result === 'object') return result.output
  const _typeGuard: never = result
  throw new Error('unreachable')
}

export interface ConfigFilesData {
  globalRc: Record<string, unknown> | undefined
  globalYaml: Record<string, unknown> | undefined
  localRc: Record<string, unknown> | undefined
  localYaml: Record<string, unknown> | undefined
}

export function readConfigFiles (globalConfigDir: string | undefined, localDir: string | undefined): ConfigFilesData {
  function tryRead<Return> (reader: () => Return): Return | undefined {
    try {
      return reader()
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
        return undefined
      }
      throw error
    }
  }

  return {
    globalRc: globalConfigDir
      ? tryRead(() => readIniFileSync(path.join(globalConfigDir, 'rc')) as Record<string, unknown>)
      : undefined,
    globalYaml: globalConfigDir
      ? tryRead(() => readYamlFileSync(path.join(globalConfigDir, 'config.yaml')))
      : undefined,
    localRc: localDir
      ? tryRead(() => readIniFileSync(path.join(localDir, '.npmrc')) as Record<string, unknown>)
      : undefined,
    localYaml: localDir
      ? tryRead(() => readYamlFileSync(path.join(localDir, 'pnpm-workspace.yaml')))
      : undefined,
  }
}

export function writeConfigFiles (globalConfigDir: string | undefined, localDir: string | undefined, data: ConfigFilesData): void {
  if (globalConfigDir) {
    fs.mkdirSync(globalConfigDir, { recursive: true })

    if (data.globalRc) {
      writeIniFileSync(path.join(globalConfigDir, 'rc'), data.globalRc)
    }

    if (data.globalYaml) {
      writeYamlFileSync(path.join(globalConfigDir, 'config.yaml'), data.globalYaml)
    }
  }

  if (localDir) {
    fs.mkdirSync(localDir, { recursive: true })

    if (data.localRc) {
      writeIniFileSync(path.join(localDir, '.npmrc'), data.localRc)
    }

    if (data.localYaml) {
      writeYamlFileSync(path.join(localDir, 'pnpm-workspace.yaml'), data.localYaml)
    }
  }
}
