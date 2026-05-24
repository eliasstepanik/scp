export function getTagTools(client) {
    return [
        {
            name: 'n8n_list_tags',
            description: 'List all tags',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                return client.get('/tags', args);
            },
        },
        {
            name: 'n8n_create_tag',
            description: 'Create a new tag',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Tag name' },
                },
            },
            execute: async (args) => {
                const body = { name: args.name };
                return client.post('/tags', body);
            },
        },
        {
            name: 'n8n_get_tag',
            description: 'Get a tag by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Tag ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/tags/${args.id}`);
            },
        },
        {
            name: 'n8n_update_tag',
            description: 'Update a tag by ID',
            inputSchema: {
                type: 'object',
                required: ['id', 'name'],
                properties: {
                    id: { type: 'string', description: 'Tag ID' },
                    name: { type: 'string', description: 'New tag name' },
                },
            },
            execute: async (args) => {
                const body = { name: args.name };
                return client.put(`/tags/${args.id}`, body);
            },
        },
        {
            name: 'n8n_delete_tag',
            description: 'Delete a tag by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Tag ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/tags/${args.id}`);
            },
        },
    ];
}
