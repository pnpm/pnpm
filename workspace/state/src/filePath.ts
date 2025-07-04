import path from 'node:path'

export const getFilePath = (workspaceDir: string): string =>
  path.join(workspaceDir, 'node_modules', '.pnpm-workspace-state.json')
