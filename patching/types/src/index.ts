export interface PatchFile {
  path: string
  hash: string
}

export interface PatchInfo {
  strict: boolean // TODO: remove this once the next major version is released
  file: PatchFile
}
