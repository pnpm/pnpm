import { createReadPackageHook } from '../lib/createReadPackageHook'

test('createReadPackageHook() is passing directory to all hooks', async () => {
  const hook1 = jest.fn((manifest) => manifest)
  const hook2 = jest.fn((manifest) => manifest)
  const readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: true,
    lockfileDir: '/foo',
    readPackageHook: [hook1, hook2],
  })
  const manifest = {}
  const dir = '/bar'
  await readPackageHook!(manifest, dir)
  expect(hook1).toBeCalledWith(manifest, dir)
  expect(hook2).toBeCalledWith(manifest, dir)
})
