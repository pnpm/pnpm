export {
  type ExtendedPatchInfo,
  type PatchFile,
  type PatchInfo,
  type PatchGroup,
  type PatchGroupRangeItem,
  type PatchGroupRecord,
} from '@pnpm/patching.types'
export { groupPatchedDependencies } from './groupPatchedDependencies'
export { getPatchInfo } from './getPatchInfo'
export { type VerifyPatchesOptions, verifyPatches } from './verifyPatches'
