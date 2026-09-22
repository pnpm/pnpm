export function makePackageManagerEnv (env: NodeJS.ProcessEnv): Record<string, string> {
  const nodeExecPath = env.NODE || process.execPath
  return {
    NODE: nodeExecPath,
    npm_node_execpath: nodeExecPath,
    // A pkg bundle's JavaScript entry point is inside a virtual filesystem.
    npm_execpath: (process as { pkg?: unknown }).pkg != null
      ? process.execPath
      : process.argv[1] || process.cwd(),
    INIT_CWD: process.cwd(),
  }
}
