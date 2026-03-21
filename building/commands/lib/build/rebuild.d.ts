import { type Config } from '@pnpm/config.reader';
import type { LogBase } from '@pnpm/logger';
import { type CreateStoreControllerOptions } from '@pnpm/store.connection-manager';
export declare function rcOptionsTypes(): Record<string, unknown>;
export declare function cliOptionsTypes(): Record<string, unknown>;
export declare const commandNames: string[];
export declare function help(): string;
export type RebuildCommandOpts = Pick<Config, 'allProjects' | 'dir' | 'engineStrict' | 'hooks' | 'lockfileDir' | 'nodeLinker' | 'rawLocalConfig' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'registries' | 'scriptShell' | 'selectedProjectsGraph' | 'sideEffectsCache' | 'sideEffectsCacheReadonly' | 'scriptsPrependNodePath' | 'shellEmulator' | 'workspaceDir'> & CreateStoreControllerOptions & {
    recursive?: boolean;
    reporter?: (logObj: LogBase) => void;
    pending: boolean;
    skipIfHasSideEffectsCache?: boolean;
    neverBuiltDependencies?: string[];
    allowBuilds?: Record<string, boolean | string>;
};
export declare function handler(opts: RebuildCommandOpts, params: string[]): Promise<void>;
