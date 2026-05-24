import type { N8nClient } from '../client.js';
export declare function getDataTableTools(client: N8nClient): ({
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
            columns?: undefined;
            dataTableId?: undefined;
            rows?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            columns: {
                type: string;
                items: {
                    type: string;
                    properties: {
                        name: {
                            type: string;
                        };
                        type: {
                            type: string;
                        };
                    };
                };
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            dataTableId?: undefined;
            rows?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            columns?: undefined;
            rows?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            columns?: undefined;
            rows?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
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
            columns?: undefined;
            rows?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            rows: {
                type: string;
                items: {
                    type: string;
                    properties?: undefined;
                };
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            columns?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            rows: {
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
            name?: undefined;
            columns?: undefined;
            type?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            type: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            columns?: undefined;
            rows?: undefined;
            columnId?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            columnId: {
                type: string;
                description: string;
            };
            name: {
                type: string;
                description: string;
            };
            type: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            columns?: undefined;
            rows?: undefined;
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
            dataTableId: {
                type: string;
                description: string;
            };
            columnId: {
                type: string;
                description: string;
            };
            limit?: undefined;
            cursor?: undefined;
            name?: undefined;
            columns?: undefined;
            rows?: undefined;
            type?: undefined;
        };
    };
    execute: (args: Record<string, unknown>) => Promise<unknown>;
})[];
