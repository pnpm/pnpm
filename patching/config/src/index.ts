export {
  type ExtendedPatchInfo,
  type PatchFile,
  type PatchInfo,
  type PatchGroup,
  type PatchGroupRangeItem,
  type PatchGroupRecord,
} from '@pnpm/patching.types'
export { groupPatchedDependencies } from './groupPatchedDependencies.js'
export { getPatchInfo } from './getPatchInfo.js'
export { type VerifyPatchesOptions, verifyPatches } from './verifyPatches.js'
