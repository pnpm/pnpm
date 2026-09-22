import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import {
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

test.each(['changed', 'deleted', 'directory', 'excluded', 'direct', 'fetch'])('frozen install verifies a %s local tarball from a warm store', async (mutation) => {
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
  const parentDir = path.resolve('parent')
  fs.mkdirSync(parentDir)
  fs.writeFileSync(path.join(parentDir, 'package.json'), JSON.stringify({
    name: 'parent',
    version: '1.0.0',
    dependencies: { 'local-tarball': `file:${tarball}` },
  }))
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: parentDir, expectSuccess: true })
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'project',
    version: '1.0.0',
    [mutation === 'excluded' ? 'devDependencies' : 'dependencies']: mutation === 'direct'
      ? { 'local-tarball': 'file:./local-tarball.tgz' }
      : { parent: 'file:./parent-1.0.0.tgz' },
  }))

  await execPnpm(['install'])

  if (['changed', 'direct', 'fetch'].includes(mutation)) {
    fs.writeFileSync(path.join(packageDir, 'index.js'), "module.exports = 'second'\n")
    packLocalTarball(packageDir, tarball)
  } else {
    fs.rmSync(tarball)
    if (mutation === 'directory') fs.mkdirSync(tarball)
  }

  if (mutation === 'fetch') {
    fs.writeFileSync('package.json', JSON.stringify({ name: 'project', version: '1.0.0' }))
    fs.writeFileSync('pnpm-lock.yaml', fs.readFileSync('pnpm-lock.yaml', 'utf8').replace('\n  .:', '\n  packages/app:'))
  }
  const { status, stdout, stderr } = execPnpmSync(mutation === 'fetch'
    ? ['fetch']
    : ['install', '--frozen-lockfile', ...(mutation === 'excluded' ? ['--prod'] : [])], { env: { CI: 'true' } })
  if (mutation === 'excluded') {
    expect(status).toBe(0)
    return
  }
  expect(status).not.toBe(0)
  expect(stdout.toString() + stderr.toString()).toContain(['changed', 'direct', 'fetch'].includes(mutation) ? 'ERR_PNPM_TARBALL_INTEGRITY' : tarball)
})

function packLocalTarball (packageDir: string, tarball: string): void {
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: packageDir, expectSuccess: true })
  fs.rmSync(tarball, { force: true })
  fs.renameSync(path.join(packageDir, '..', 'local-tarball-1.0.0.tgz'), tarball)
}
