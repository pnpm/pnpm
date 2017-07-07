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

declare module 'is-ci' {
  const anything: any;
  export = anything;
}

declare module 'js-yaml' {
  const anything: any;
  export = anything;
}

declare module 'write-file-atomic' {
  const anything: any;
  export = anything;
}

declare module 'thenify' {
  const anything: any;
  export = anything;
}
