import { fileURLToPath } from 'node:url'

import { parseShell } from '@yarnpkg/parsers'
import { globUtils } from '@yarnpkg/shell'

import { spawnProcess } from './process.mjs'

export function spawnShell (resources, options, configuration) {
  try {
    parseShell(options.script, globUtils)
  } catch (error) {
    throw Object.assign(new Error(error.message, { cause: error }), { code: 'ESHELLPARSE' })
  }
  return spawnProcess(resources, {
    ...options,
    program: process.execPath,
    args: [fileURLToPath(new URL('./shell-runner.mjs', import.meta.url)), options.script, ...(options.args ?? [])],
  }, configuration)
}
