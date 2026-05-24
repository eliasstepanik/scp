import type { N8nClient } from '../client.js';
export declare function getCredentialTools(client: N8nClient): ({
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
            includeData: {
                type: string;
                description: string;
            };
            type: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            data?: undefined;
            id?: undefined;
            credentialTypeName?: undefined;
            destinationProjectId?: undefined;
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
            type: {
                type: string;
                description: string;
            };
            data: {
                type: string;
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeData?: undefined;
            id?: undefined;
            credentialTypeName?: undefined;
            destinationProjectId?: undefined;
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
            includeData: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            type?: undefined;
            name?: undefined;
            projectId?: undefined;
            data?: undefined;
            credentialTypeName?: undefined;
            destinationProjectId?: undefined;
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
            data: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeData?: undefined;
            type?: undefined;
            projectId?: undefined;
            credentialTypeName?: undefined;
            destinationProjectId?: undefined;
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
            includeData?: undefined;
            type?: undefined;
            name?: undefined;
            projectId?: undefined;
            data?: undefined;
            credentialTypeName?: undefined;
            destinationProjectId?: undefined;
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
            credentialTypeName: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeData?: undefined;
            type?: undefined;
            name?: undefined;
            projectId?: undefined;
            data?: undefined;
            id?: undefined;
            destinationProjectId?: undefined;
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
            destinationProjectId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            includeData?: undefined;
            type?: undefined;
            name?: undefined;
            projectId?: undefined;
            data?: undefined;
            credentialTypeName?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
