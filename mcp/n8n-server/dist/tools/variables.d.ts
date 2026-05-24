import type { N8nClient } from '../client.js';
export declare function getVariableTools(client: N8nClient): ({
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
            key?: undefined;
            value?: undefined;
            type?: undefined;
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
            key: {
                type: string;
                description: string;
            };
            value: {
                type: string;
                description: string;
            };
            type: {
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
            key: {
                type: string;
                description: string;
            };
            value: {
                type: string;
                description: string;
            };
            type: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
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
            key?: undefined;
            value?: undefined;
            type?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
