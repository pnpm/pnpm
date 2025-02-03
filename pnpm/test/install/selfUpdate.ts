import fs from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { STORE_VERSION } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import isWindows from 'is-windows'
import {
  execPnpm,
  retryLoadJsonFile,
  spawnPnpm,
} from '../utils'

const skipOnWindows = isWindows() ? test.skip : test

skipOnWindows('self-update stops the store server', async () => {
  const project = prepare()

  spawnPnpm(['server', 'start'])

  const serverJsonPath = path.resolve(`../store/${STORE_VERSION}/server/server.json`)
  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions).toBeTruthy()

  const pnpmHome = process.cwd()

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: path.resolve('data'),
  }

  await execPnpm(['self-update', `--config.store-dir=${path.resolve('..', 'store')}`, '--reporter=append-only', '9.15.5'], { env })

  expect(fs.existsSync(serverJsonPath)).toBeFalsy()
  project.isExecutable('../pnpm')
})
