import path from 'path'

export const getCacheFilePath = (workspaceDir: string): string =>
  path.join(workspaceDir, 'node_modules', '.workspace-packages-list.json')
