import tape = require('tape')
import promisifyTape from 'tape-promise'
import exists = require('path-exists')
import {
  execPnpm,
  preparePackages,
} from '../utils'

const test = promisifyTape(tape)

test('recursive linking/unlinking', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'link')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.devDependencies['is-positive'], 'link:../is-positive')
  }

  await execPnpm('recursive', 'unlink')

  process.chdir('project-1')
  t.ok(await exists('node_modules', 'is-positive', 'index.js'), 'local package is unlinked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.registry, 'http://localhost:4873/', 'project-1 has correct registry specified in shrinkwrap.yaml')
    t.equal(project1Shr.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Shr.packages['/is-positive/1.0.0'])
  }

  const isPositiveShr = await projects['is-positive'].loadShrinkwrap()
  t.equal(isPositiveShr.registry, 'http://localhost:4873/', 'is-positive has correct registry specified in shrinkwrap.yaml')
})

test('recursive unlink specific package', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'link')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.devDependencies['is-positive'], 'link:../is-positive')
  }

  await execPnpm('recursive', 'unlink', 'is-positive')

  process.chdir('project-1')
  t.ok(await exists('node_modules', 'is-positive', 'index.js'), 'local package is unlinked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.registry, 'http://localhost:4873/', 'project-1 has correct registry specified in shrinkwrap.yaml')
    t.equal(project1Shr.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Shr.packages['/is-positive/1.0.0'])
  }

  const isPositiveShr = await projects['is-positive'].loadShrinkwrap()
  t.equal(isPositiveShr.registry, 'http://localhost:4873/', 'is-positive has correct registry specified in shrinkwrap.yaml')
})
