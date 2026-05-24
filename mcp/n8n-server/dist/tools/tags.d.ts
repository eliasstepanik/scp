import type { N8nClient } from '../client.js';
export declare function getTagTools(client: N8nClient): ({
    name: string;
    description: string;
    inputSchema: {
        type: string;
        properties: {
            limit: {
                type: string;
                description: string;
            };
            cursor: {
                type: string;
                description: string;
            };
            name?: undefined;
            id?: undefined;
        };
        required?: undefined;
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
} | {
    name: string;
    description: string;
    inputSchema: {
        type: string;
        required: string[];
        properties: {
            name: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            id?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
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
            id: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
