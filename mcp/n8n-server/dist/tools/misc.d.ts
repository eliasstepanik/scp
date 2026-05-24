import type { N8nClient } from '../client.js';
export declare function getMiscTools(client: N8nClient): ({
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            projectId?: undefined;
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            parentFolderId?: undefined;
            folderId?: undefined;
        };
        required?: undefined;
    };
    execute: (_args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            limit: {
                type: string;
                description: string;
            };
            cursor: {
                type: string;
                description: string;
            };
            name?: undefined;
            parentFolderId?: undefined;
            folderId?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            parentFolderId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            folderId?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            folderId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            parentFolderId?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            projectId: {
                type: string;
                description: string;
            };
            folderId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            parentFolderId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
