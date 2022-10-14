import path from 'path'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { prepare } from '@pnpm/prepare'
import exists from 'path-exists'
import PATH from 'path-name'
import { execPnpm } from './utils'

test('uninstall package and remove from appropriate property', async () => {
  const project = prepare()
  await execPnpm(['install', '--save-optional', 'is-positive@3.1.0'])

  // testing the CLI directly as there was an issue where `npm.config` started to set save = true by default
  // npm@5 introduced --save-prod that bahaves the way --save worked in pre 5 versions
  await execPnpm(['uninstall', 'is-positive'])

  await project.storeHas('is-positive', '3.1.0')

  await execPnpm(['store', 'prune'])

  await project.storeHasNot('is-positive', '3.1.0')

  await project.hasNot('is-positive')

  const pkgJson = await readPackageJsonFromDir(process.cwd())
  expect(pkgJson.optionalDependencies).toBeUndefined()
})

test('uninstall global package with its bin files', async () => {
  prepare()

  const global = process.cwd()
  const globalBin = path.resolve(global, 'bin')

  const env = {
    PNPM_HOME: globalBin,
    [PATH]: `${globalBin}${path.delimiter}${process.env[PATH] ?? ''}`,
    XDG_DATA_HOME: global,
  }

  await execPnpm(['add', '-g', '@pnpm.e2e/sh-hello-world@1.0.1'], { env })

  let stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  expect(stat).toBeTruthy() // sh-hello-world is in .bin

  await execPnpm(['uninstall', '-g', '@pnpm.e2e/sh-hello-world'], { env })

  stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  expect(stat).toBeFalsy() // sh-hello-world is removed from .bin
})
