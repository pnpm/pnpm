export interface PatchFile {
  path: string
  hash: string
}

// TODO: replace all occurrences of PatchInfo with PatchFile before the next major version is released
export interface PatchInfo {
  strict: boolean
  file: PatchFile
}
