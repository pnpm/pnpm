import path from 'path'
import PATH_NAME from 'path-name'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { execPnpm } from '../utils/index.js'

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
