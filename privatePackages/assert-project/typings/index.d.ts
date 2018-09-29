declare module 'load-yaml-file' {
  interface LoadYamlFile {
    <T>(filepath: string): Promise<T>
    sync<T>(filepath: string): T
  }

  const loadYamlFile: LoadYamlFile

  export = loadYamlFile;
}

declare module 'is-windows' {
  function isWindows(): boolean;
  export = isWindows;
}

declare module 'isexe' {
  const anything: any;
  export = anything;
}

declare module 'util.promisify' {
  const anything: any;
  export = anything;
}
