import prepare from '@pnpm/prepare'
import deepRequireCwd from 'deep-require-cwd'
import { execPnpm } from '../utils'

test('installing optional dependencies when --no-optional is not used', async () => {
  const project = prepare({
    dependencies: {
      'pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install'])

  await project.has('is-positive')
  await project.has('pkg-with-good-optional')

  expect(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json'])).toBeTruthy()
  expect(deepRequireCwd(['pkg-with-good-optional', 'is-positive', './package.json'])).toBeTruthy()
})

test('not installing optional dependencies when --no-optional is used', async () => {
  const project = prepare({
    dependencies: {
      'pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install', '--no-optional'])

  await project.hasNot('is-positive')
  await project.has('pkg-with-good-optional')

  expect(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json'])).toBeTruthy()
  expect(deepRequireCwd.silent(['pkg-with-good-optional', 'is-positive', './package.json'])).toBeFalsy()
})
