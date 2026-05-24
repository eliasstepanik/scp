import type { N8nClient } from '../client.js';
export declare function getWorkflowTools(client: N8nClient): ({
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
            active: {
                type: string;
                description: string;
            };
            tags: {
                type: string;
                description: string;
                items?: undefined;
            };
            name: {
                type: string;
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            excludePinnedData: {
                type: string;
                description: string;
            };
            onlyActive: {
                type: string;
                description: string;
            };
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
            id?: undefined;
            versionId?: undefined;
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
            nodes: {
                type: string;
                description: string;
            };
            connections: {
                type: string;
                description: string;
            };
            settings: {
                type: string;
                description: string;
            };
            staticData: {
                type: string;
                description: string;
            };
            tags: {
                type: string;
                items: {
                    type: string;
                    properties?: undefined;
                };
                description: string;
            };
            projectId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            active?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            id?: undefined;
            versionId?: undefined;
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
            excludePinnedData: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            active?: undefined;
            tags?: undefined;
            name?: undefined;
            projectId?: undefined;
            onlyActive?: undefined;
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
            versionId?: undefined;
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
            versionId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            active?: undefined;
            tags?: undefined;
            name?: undefined;
            projectId?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
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
            nodes: {
                type: string;
                description: string;
            };
            connections: {
                type: string;
                description: string;
            };
            settings: {
                type: string;
                description: string;
            };
            staticData: {
                type: string;
                description: string;
            };
            tags: {
                type: string;
                items: {
                    type: string;
                    properties?: undefined;
                };
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            active?: undefined;
            projectId?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            versionId?: undefined;
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
            active?: undefined;
            tags?: undefined;
            name?: undefined;
            projectId?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
            versionId?: undefined;
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
            active?: undefined;
            tags?: undefined;
            name?: undefined;
            projectId?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
            versionId?: undefined;
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
            tags: {
                type: string;
                items: {
                    type: string;
                    properties: {
                        id: {
                            type: string;
                        };
                    };
                };
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            active?: undefined;
            name?: undefined;
            projectId?: undefined;
            excludePinnedData?: undefined;
            onlyActive?: undefined;
            nodes?: undefined;
            connections?: undefined;
            settings?: undefined;
            staticData?: undefined;
            versionId?: undefined;
            destinationProjectId?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
