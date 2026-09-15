import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

const configDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-config-'))

try {
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => {
    const lowerName = name.toLowerCase()
    return !lowerName.startsWith('npm_config_') && !lowerName.startsWith('pnpm_config_')
  }))
  const npmrcPath = path.join(configDir, 'npmrc')
  fs.writeFileSync(npmrcPath, '')
  Object.assign(env, {
    PNPM_CONFIG_CI: 'false',
    PNPM_CONFIG_NPMRC_AUTH_FILE: npmrcPath,
    XDG_CONFIG_HOME: configDir,
  })

  const result = spawnSync('cargo', ['nextest', 'run', ...process.argv.slice(2)], {
    env,
    stdio: 'inherit',
  })
  if (result.error != null) throw result.error
  process.exitCode = result.status ?? 1
} finally {
  fs.rmSync(configDir, { recursive: true, force: true })
}
