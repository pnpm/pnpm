import {FakeFS, PortablePath} from '@yarnpkg/fslib';
import {readFile}             from 'fs';
import {dirname}              from 'path';
import {promisify}            from 'util';

import {hydrateRuntimeState}  from './loader/hydrateRuntimeState';
import {makeApi}              from './loader/makeApi';
import {SerializedState}      from './types';

const readFileP = promisify(readFile);

// Note that using those functions is typically NOT needed! The PnP API is
// designed to be consumed directly from within Node - meaning that depending
// on your situation you probably should use one of those two alternatives
// instead:
//
//   - If your script is executing within a PnP environment, you'll be able to
//     simply `require("pnpapi")` in order to get a reference to the running
//     API. You can also simply check whether you're actually running within a
//     PnP environment by checking `process.versions.pnp`.
//
//   - Or if you're not running within a PnP environment, or wish to interact
//     with a different one than the current one, then you can directly require
//     its `.pnp.cjs` file.
//
// The function exported in this file only work when the PnP data are kept
// outside of the loader (pnpEnableInlining = false in Yarn), and their only
// real use case is to access the PnP API without running the risk of executing
// third-party Javascript code.

export async function hydratePnpFile(location: string, {fakeFs, pnpapiResolution}: {fakeFs: FakeFS<PortablePath>, pnpapiResolution: string}) {
  const source = await readFileP(location, `utf8`);

  return hydratePnpSource(source, {
    basePath: dirname(location),
    fakeFs,
    pnpapiResolution,
  });
}

export function hydratePnpSource(source: string, {basePath, fakeFs, pnpapiResolution}: {basePath: string, fakeFs: FakeFS<PortablePath>, pnpapiResolution: string}) {
  const data = JSON.parse(source) as SerializedState;

  const runtimeState = hydrateRuntimeState(data, {
    basePath,
  });

  return makeApi(runtimeState, {
    compatibilityMode: true,
    fakeFs,
    pnpapiResolution,
  });
}
