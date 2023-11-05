export function filesIncludeInstallScripts (filesIndex: Record<string, unknown>): boolean {
  return filesIndex['binding.gyp'] != null ||
    Object.keys(filesIndex).some((filename) => !(filename.match(/^[.]hooks[\\/]/) == null)) // TODO: optimize this
}
