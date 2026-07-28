import { promises as fs } from 'node:fs'
import path from 'node:path'

import { rimraf } from '@zkochan/rimraf'

/**
 * The alias must be validated as a dependency name before calling this function.
 */
export async function removeObsoleteDependency (modulesDir: string, alias: string): Promise<void> {
  await rimraf(path.join(modulesDir, alias))
  if (alias[0] === '@') {
    await fs.rmdir(path.join(modulesDir, alias.split('/')[0])).catch(() => {})
  }
}
