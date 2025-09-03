import eslintConfig from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

export default [{
    ignores: ["**/fixtures", "**/__fixtures__"],
}, ...eslintConfig, regexpPlugin.configs['flat/recommended']];
