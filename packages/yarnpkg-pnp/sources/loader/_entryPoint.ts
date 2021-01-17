import {FakeFS, NodeFS, NativePath, PortablePath, VirtualFS, ZipOpenFS} from '@yarnpkg/fslib';
import {getLibzipSync}                                                  from '@yarnpkg/libzip';
import fs                                                               from 'fs';
import Module                                                           from 'module';
import StringDecoder                                                    from 'string_decoder';

import {RuntimeState, PnpApi}                                           from '../types';

import {applyPatch}                                                     from './applyPatch';
import {hydrateRuntimeState}                                            from './hydrateRuntimeState';
import {MakeApiOptions, makeApi}                                        from './makeApi';
import {Manager, makeManager}                                           from './makeManager';

declare var __non_webpack_module__: NodeModule;
declare var $$SETUP_STATE: (hrs: typeof hydrateRuntimeState, basePath?: NativePath) => RuntimeState;

// We must copy the fs into a local, because otherwise
// 1. we would make the NodeFS instance use the function that we patched (infinite loop)
// 2. Object.create(fs) isn't enough, since it won't prevent the proto from being modified
const localFs: typeof fs = {...fs};
const nodeFs = new NodeFS(localFs);

const defaultRuntimeState = $$SETUP_STATE(hydrateRuntimeState);
const defaultPnpapiResolution = __filename;

// We create a virtual filesystem that will do three things:
// 1. all requests inside a folder named "$$virtual" will be remapped according the virtual folder rules
// 2. all requests going inside a Zip archive will be handled by the Zip fs implementation
// 3. any remaining request will be forwarded to Node as-is
const defaultFsLayer: FakeFS<PortablePath> = new VirtualFS({
  baseFs: new ZipOpenFS({
    baseFs: nodeFs,
    libzip: getLibzipSync(),
    maxOpenFiles: 80,
    readOnlyArchives: true,
  }),
});

let manager: Manager;

const defaultApi = Object.assign(makeApi(defaultRuntimeState, {
  fakeFs: defaultFsLayer,
  pnpapiResolution: defaultPnpapiResolution,
}), {
  /**
   * Can be used to generate a different API than the default one (for example
   * to map it on `/` rather than the local directory path, or to use a
   * different FS layer than the default one).
   */
  makeApi: ({
    basePath = undefined,
    fakeFs = defaultFsLayer,
    pnpapiResolution = defaultPnpapiResolution,
    ...rest
  }: Partial<MakeApiOptions> & {basePath?: NativePath}) => {
    const apiRuntimeState = typeof basePath !== `undefined`
      ? $$SETUP_STATE(hydrateRuntimeState, basePath)
      : defaultRuntimeState;

    return makeApi(apiRuntimeState, {
      fakeFs,
      pnpapiResolution,
      ...rest,
    });
  },

  /**
   * Will inject the specified API into the environment, monkey-patching FS. Is
   * automatically called when the hook is loaded through `--require`.
   */
  setup: (api?: PnpApi) => {
    applyPatch(api || defaultApi, {
      fakeFs: defaultFsLayer,
      manager,
    });
  },
});

manager = makeManager(defaultApi, {
  fakeFs: defaultFsLayer,
});

// eslint-disable-next-line arca/no-default-export
export default defaultApi;

if (__non_webpack_module__.parent && __non_webpack_module__.parent.id === `internal/preload`) {
  defaultApi.setup();

  if (__non_webpack_module__.filename) {
    // We delete it from the cache in order to support the case where the CLI resolver is invoked from "yarn run"
    // It's annoying because it might cause some issues when the file is multiple times in NODE_OPTIONS, but it shouldn't happen anyway.

    delete Module._cache[__non_webpack_module__.filename];
  }
}

if (process.mainModule === __non_webpack_module__) {
  const reportError = (code: string, message: string, data: Object) => {
    process.stdout.write(`${JSON.stringify([{code, message, data}, null])}\n`);
  };

  const reportSuccess = (resolution: string | null) => {
    process.stdout.write(`${JSON.stringify([null, resolution])}\n`);
  };

  const processResolution = (request: string, issuer: string) => {
    try {
      reportSuccess(defaultApi.resolveRequest(request, issuer));
    } catch (error) {
      reportError(error.code, error.message, error.data);
    }
  };

  const processRequest = (data: string) => {
    try {
      const [request, issuer] = JSON.parse(data);
      processResolution(request, issuer);
    } catch (error) {
      reportError(`INVALID_JSON`, error.message, error.data);
    }
  };

  if (process.argv.length > 2) {
    if (process.argv.length !== 4) {
      process.stderr.write(`Usage: ${process.argv[0]} ${process.argv[1]} <request> <issuer>\n`);
      process.exitCode = 64; /* EX_USAGE */
    } else {
      processResolution(process.argv[2], process.argv[3]);
    }
  } else {
    let buffer = ``;
    const decoder = new StringDecoder.StringDecoder();

    process.stdin.on(`data`, chunk => {
      buffer += decoder.write(chunk);

      do {
        const index = buffer.indexOf(`\n`);
        if (index === -1)
          break;

        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);

        processRequest(line);
      } while (true);
    });
  }
}
