import { expect, test } from '@jest/globals'
import type { PackageSnapshots } from '@pnpm/lockfile.utils'
import { DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'
import type { DepPath, ProjectManifest } from '@pnpm/types'

import { getPreferredVersionsFromLockfileAndManifests } from '../lib/index.js'

test('seeds manifest specs and lockfile pins with combined weights', () => {
  const manifest: ProjectManifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      foo: '1.0.0',
      bar: '^2.0.0',
    },
  }
  const snapshots: PackageSnapshots = {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-0' } },
    ['qar@3.0.0' as DepPath]: { resolution: { integrity: 'sha512-1' } },
  }

  const preferredVersions = getPreferredVersionsFromLockfileAndManifests(snapshots, [manifest])

  // Manifest pin that is also locked gets both weights added together.
  expect(preferredVersions.foo['1.0.0']).toEqual({ selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT + DIRECT_DEP_SELECTOR_WEIGHT })
  // Manifest range: manifest weight only.
  expect(preferredVersions.bar['^2.0.0']).toEqual({ selectorType: 'range', weight: DIRECT_DEP_SELECTOR_WEIGHT })
  // Lockfile-only pin: lockfile weight only.
  expect(preferredVersions.qar['3.0.0']).toEqual({ selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT })
})

test('catalog: specs seed the catalog entry they name', () => {
  const manifest: ProjectManifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      'from-default': 'catalog:',
      'from-named': 'catalog:tools',
      missing: 'catalog:',
    },
  }

  const preferredVersions = getPreferredVersionsFromLockfileAndManifests(undefined, [manifest], {
    catalogs: {
      default: { 'from-default': '1.2.3' },
      tools: { 'from-named': '^2.0.0' },
    },
  })

  expect(preferredVersions['from-default']).toEqual({ '1.2.3': { selectorType: 'version', weight: DIRECT_DEP_SELECTOR_WEIGHT } })
  expect(preferredVersions['from-named']).toEqual({ '^2.0.0': { selectorType: 'range', weight: DIRECT_DEP_SELECTOR_WEIGHT } })
  expect(preferredVersions.missing).toBeUndefined()
})

test('a dependency named __proto__ cannot pollute Object.prototype', () => {
  // Manifests and lockfiles are attacker-controlled; JSON.parse produces own
  // `__proto__` keys just like a crafted package.json / pnpm-lock.yaml does.
  const manifest = JSON.parse(`{
    "name": "project",
    "version": "1.0.0",
    "dependencies": { "__proto__": "1.0.0", "constructor": "^2.0.0" }
  }`) as ProjectManifest
  const snapshots = JSON.parse(`{
    "__proto__@1.0.0": { "resolution": { "integrity": "sha512-0" } }
  }`) as PackageSnapshots

  const preferredVersions = getPreferredVersionsFromLockfileAndManifests(snapshots, [manifest])

  // The crafted names land as plain own keys with the usual weights...
  expect(Object.getOwnPropertyDescriptor(preferredVersions, '__proto__')?.value)
    .toEqual({ '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT + DIRECT_DEP_SELECTOR_WEIGHT } })
  expect(Object.getOwnPropertyDescriptor(preferredVersions, 'constructor')?.value)
    .toEqual({ '^2.0.0': { selectorType: 'range', weight: DIRECT_DEP_SELECTOR_WEIGHT } })
  // ...and Object.prototype is untouched.
  expect(({} as Record<string, unknown>)['1.0.0']).toBeUndefined()
  expect(Object.prototype).not.toHaveProperty('1.0.0')
})
