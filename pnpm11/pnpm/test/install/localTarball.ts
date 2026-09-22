import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import {
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

test('frozen install rejects a changed local tarball from a warm store', async () => {
  prepareEmpty()
  const packageDir = path.resolve('local-tarball')
  const tarball = path.resolve('local-tarball.tgz')
  fs.mkdirSync(packageDir)
  fs.writeFileSync(path.join(packageDir, 'package.json'), JSON.stringify({
    name: 'local-tarball',
    version: '1.0.0',
  }))
  fs.writeFileSync(path.join(packageDir, 'index.js'), "module.exports = 'first'\n")
  packLocalTarball(packageDir, tarball)
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'project',
    version: '1.0.0',
    dependencies: {
      'local-tarball': 'file:./local-tarball.tgz',
    },
  }))

  await execPnpm(['install'])

  fs.writeFileSync(path.join(packageDir, 'index.js'), "module.exports = 'second'\n")
  packLocalTarball(packageDir, tarball)

  const { status, stderr } = execPnpmSync(['install', '--frozen-lockfile'])
  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('ERR_PNPM_TARBALL_INTEGRITY')
})

function packLocalTarball (packageDir: string, tarball: string): void {
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: packageDir, expectSuccess: true })
  fs.rmSync(tarball, { force: true })
  fs.renameSync(path.join(packageDir, '..', 'local-tarball-1.0.0.tgz'), tarball)
}
