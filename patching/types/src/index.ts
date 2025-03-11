export interface PatchFile {
  path: string
  hash: string
}

// TODO: replace all occurrences of PatchInfo with PatchFile before the next major version is released
export interface PatchInfo {
  strict: boolean
  file: PatchFile
}

export interface ExtendedPatchInfo extends PatchInfo {
  key: string
}

/** A group of {@link ExtendedPatchInfo}s which correspond to a package name. */
export interface PatchGroup {
  /** Maps exact versions to {@link ExtendedPatchInfo}. */
  exact: Record<string, ExtendedPatchInfo>
  /** Maps version ranges to {@link ExtendedPatchInfo}. */
  range: Record<string, ExtendedPatchInfo>
  /** The {@link ExtendedPatchInfo} without exact versions or version ranges. */
  all: ExtendedPatchInfo | undefined
}

/** Maps package names to their corresponding groups. */
export type PatchGroupRecord = Record<string, PatchGroup>
