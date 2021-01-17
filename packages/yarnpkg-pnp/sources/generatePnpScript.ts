import {generatePrettyJson}      from './generatePrettyJson';
import {generateSerializedState} from './generateSerializedState';
// @ts-expect-error
import getTemplate               from './hook';
import {SerializedState}         from './types';
import {PnpSettings}             from './types';

function generateLoader(shebang: string | null | undefined, loader: string) {
  return [
    shebang ? `${shebang}\n` : ``,
    `/* eslint-disable */\n\n`,
    `try {\n`,
    `  Object.freeze({}).detectStrictMode = true;\n`,
    `} catch (error) {\n`,
    `  throw new Error(\`The whole PnP file got strict-mode-ified, which is known to break (Emscripten libraries aren't strict mode). This usually happens when the file goes through Babel.\`);\n`,
    `}\n`,
    `\n`,
    `var __non_webpack_module__ = module;\n`,
    `\n`,
    `function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n`,
    loader.replace(/^/gm, `  `),
    `}\n`,
    `\n`,
    getTemplate(),
  ].join(``);
}

function generateJsonString(data: SerializedState) {
  return JSON.stringify(data, null, 2);
}

function generateInlinedSetup(data: SerializedState) {
  return [
    `return hydrateRuntimeState(${generatePrettyJson(data)}, {basePath: basePath || __dirname});\n`,
  ].join(``);
}

function generateSplitSetup(dataLocation: string) {
  return [
    `var path = require('path');\n`,
    `var dataLocation = path.resolve(__dirname, ${JSON.stringify(dataLocation)});\n`,
    `return hydrateRuntimeState(require(dataLocation), {basePath: basePath || path.dirname(dataLocation)});\n`,
  ].join(``);
}

export function generateInlinedScript(settings: PnpSettings): string {
  const data = generateSerializedState(settings);

  const setup = generateInlinedSetup(data);
  const loaderFile = generateLoader(settings.shebang, setup);

  return loaderFile;
}

export function generateSplitScript(settings: PnpSettings & {dataLocation: string}): {dataFile: string, loaderFile: string} {
  const data = generateSerializedState(settings);

  const setup = generateSplitSetup(settings.dataLocation);
  const loaderFile = generateLoader(settings.shebang, setup);

  return {dataFile: generateJsonString(data), loaderFile};
}
