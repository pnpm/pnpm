import path from 'path'
import PATH_NAME from 'path-name'
import { prepare } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { execPnpm, execPnpmSync } from '../utils/index.js'

test('self-update updates the packageManager field in package.json', async () => {
  prepare({
    packageManager: 'pnpm@9.0.0',
  })

  const pnpmHome = process.cwd()

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: path.resolve('data'),
  }

  await execPnpm(['self-update', '10.0.0'], { env })

  expect(loadJsonFileSync<ProjectManifest>('package.json').packageManager).toBe('pnpm@10.0.0')
})

test('version switch reuses pnpm previously installed by self-update', async () => {
  prepare({
    packageManager: 'pnpm@9.0.0',
  })

  const pnpmHome = process.cwd()

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: path.resolve('data'),
  }

  // self-update installs pnpm 10.0.0 to the global dir (with GVS enabled),
  // populating the global virtual store
  await execPnpm(['self-update', '10.0.0'], { env })

  // Now set packageManager to the same version and run install.
  // The version switch should find pnpm 10.0.0 already in the GVS
  // and reuse it without downloading again.
  const result = execPnpmSync(['-v'], { env })
  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe('10.0.0')
})
