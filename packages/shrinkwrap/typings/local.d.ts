declare module 'load-yaml-file' {
  interface LoadYamlFile {
    <T>(filepath: string): Promise<T>
    sync<T>(filepath: string): T
  }

  const loadYamlFile: LoadYamlFile

  export = loadYamlFile;
}

declare module 'rimraf-then' {
  const anything: any;
  export = anything;
}

declare module 'write-file-atomic' {
  const anything: any;
  export = anything;
}

declare module 'util.promisify' {
  const anything: any;
  export = anything;
}

declare module 'mkdirp-promise' {
  const anything: any;
  export = anything;
}

declare module 'yaml-tag' {
  const anything: any;
  export = anything;
}
