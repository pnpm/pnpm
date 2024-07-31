export interface PatchFile {
  path: string
  hash: string
}

export interface PatchInfo {
  strict: boolean
  file: PatchFile
}
