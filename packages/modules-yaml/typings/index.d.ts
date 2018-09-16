declare module 'load-yaml-file' {
  interface LoadYamlFile {
    <T>(filepath: string): Promise<T>
    sync<T>(filepath: string): T
  }

  const loadYamlFile: LoadYamlFile

  export = loadYamlFile;
}
