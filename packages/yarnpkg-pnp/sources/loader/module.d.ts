import 'module';

declare module "module" {
  const _cache: {[p: string]: NodeModule};
  const _extensions: {[ext: string]: any};

  function _findPath(request: string, paths: Array<string> | null | undefined, isMain: boolean): string | false;
  function _nodeModulePaths(from: string): Array<string>;
  function _resolveFilename(request: string, parent: NodeModule | null | undefined, isMain: boolean, options?: {[key: string]: any}): string;
  function _load(request: string, parent: NodeModule | null | undefined, isMain: boolean): any;

  interface Module extends NodeModule {
    pnpApiPath?: import('@yarnpkg/fslib').PortablePath | null;
  }
}
