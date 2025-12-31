import { getOptionsFromPnpmSettings } from '../lib/getOptionsFromRootManifest.js'

const ORIGINAL_ENV = process.env

afterEach(() => {
  process.env = { ...ORIGINAL_ENV }
})

test('getOptionsFromPnpmSettings() replaces env variables in settings', () => {
  process.env.PNPM_TEST_KEY = 'foo'
  process.env.PNPM_TEST_VALUE = 'bar'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    '${PNPM_TEST_KEY}': '${PNPM_TEST_VALUE}', // eslint-disable-line
  } as any) as any // eslint-disable-line
  expect(options.foo).toBe('bar')
})

test('getOptionsFromRootManifest() converts allowBuilds', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowBuilds: {
        foo: true,
        bar: false,
        qar: 'warn',
      },
    },
  })
  expect(options).toStrictEqual({
    onlyBuiltDependencies: ['foo'],
    ignoredBuiltDependencies: ['bar'],
  })
})
