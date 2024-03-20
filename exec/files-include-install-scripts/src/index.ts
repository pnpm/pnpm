import '@total-typescript/ts-reset'

export function filesIncludeInstallScripts(
  filesIndex: Record<string, unknown>
): boolean {
  return (
    filesIndex['binding.gyp'] != null ||
    Object.keys(filesIndex).some(
      (filename: string): boolean => {
        return filename.match(/^[.]hooks[\\/]/) != null;
      }
    )
  )
}
