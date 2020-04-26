import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import ncpCB = require('ncp')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { promisify } from 'util'
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const ncp = promisify(ncpCB)

test('corrupted tarball should be redownloaded to the store', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm(['store', 'add', 'is-positive@1.0.0', 'is-positive@2.0.0'])

  await rimraf(path.resolve(`../store/v3/localhost+${REGISTRY_MOCK_PORT}/is-positive/2.0.0`))
  await fs.mkdir(path.resolve(`../store/v3/localhost+${REGISTRY_MOCK_PORT}/is-positive/2.0.0`), { recursive: true })
  await ncp(
    path.resolve(`../store/v3/localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0/packed.tgz`),
    path.resolve(`../store/v3/localhost+${REGISTRY_MOCK_PORT}/is-positive/2.0.0/packed.tgz`),
  )

  await execPnpm(['add', 'is-positive@2.0.0'])

  await project.has('is-positive')

  t.equal(project.requireModule('is-positive/package.json').version, '2.0.0', 'correct tarball redownloaded to the store')
})
