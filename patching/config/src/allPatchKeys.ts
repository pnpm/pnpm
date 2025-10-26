import { type PatchGroupRecord } from '@pnpm/patching.types'

export function * allPatchKeys (patchedDependencies: PatchGroupRecord): Generator<string> {
  for (const name in patchedDependencies) {
    const group = patchedDependencies[name]
    for (const version in group.exact) {
      yield group.exact[version].key
    }
    for (const item of group.range) {
      yield item.patch.key
    }
    if (group.all) {
      yield group.all.key
    }
  }
}
