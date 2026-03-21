import type { Config } from '@pnpm/config.reader';
import { type RebuildCommandOpts } from '../build/index.js';
export type ApproveBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'allowBuilds' | 'enableGlobalVirtualStore'> & {
    all?: boolean;
    global?: boolean;
};
export declare const commandNames: string[];
export declare function help(): string;
export declare function cliOptionsTypes(): Record<string, unknown>;
export declare function rcOptionsTypes(): Record<string, unknown>;
export declare function handler(opts: ApproveBuildsCommandOpts & RebuildCommandOpts, params?: string[]): Promise<void>;
