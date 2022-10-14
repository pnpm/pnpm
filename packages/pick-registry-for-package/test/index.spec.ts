import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'

test('pick correct scope', () => {
  const registries = {
    default: 'https://registry.npmjs.org/',
    '@private': 'https://private.registry.com/',
  }
  expect(pickRegistryForPackage(registries, '@private/lodash')).toBe('https://private.registry.com/')
  expect(pickRegistryForPackage(registries, '@random/lodash')).toBe('https://registry.npmjs.org/')
  expect(pickRegistryForPackage(registries, '@random/lodash', 'npm:@private/lodash@1')).toBe('https://private.registry.com/')
})
