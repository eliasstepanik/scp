export function getWorkflowTools(client) {
    return [
        {
            name: 'n8n_list_workflows',
            description: 'List all workflows',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                    active: { type: 'boolean', description: 'Filter by active status' },
                    tags: { type: 'string', description: 'Filter by tags (comma-separated)' },
                    name: { type: 'string', description: 'Filter by name' },
                    projectId: { type: 'string', description: 'Filter by project ID' },
                    excludePinnedData: { type: 'boolean', description: 'Exclude pinned data' },
                    onlyActive: { type: 'boolean', description: 'Only return active workflows' },
                },
            },
            execute: async (args) => {
                return client.get('/workflows', args);
            },
        },
        {
            name: 'n8n_create_workflow',
            description: 'Create a new workflow',
            inputSchema: {
                type: 'object',
                required: ['name', 'nodes', 'connections'],
                properties: {
                    name: { type: 'string', description: 'Workflow name' },
                    nodes: { type: 'array', description: 'Workflow nodes array' },
                    connections: { type: 'object', description: 'Node connections object' },
                    settings: { type: 'object', description: 'Workflow settings' },
                    staticData: { type: 'object', description: 'Static data' },
                    tags: { type: 'array', items: { type: 'string' }, description: 'Tag IDs' },
                    projectId: { type: 'string', description: 'Project to assign workflow to' },
                },
            },
            execute: async (args) => {
                return client.post('/workflows', args);
            },
        },
        {
            name: 'n8n_get_workflow',
            description: 'Get a workflow by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                    excludePinnedData: { type: 'boolean', description: 'Exclude pinned data' },
                },
            },
            execute: async (args) => {
                const { id, ...query } = args;
                return client.get(`/workflows/${id}`, query);
            },
        },
        {
            name: 'n8n_get_workflow_version',
            description: 'Get a specific version of a workflow',
            inputSchema: {
                type: 'object',
                required: ['id', 'versionId'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                    versionId: { type: 'string', description: 'Version ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/workflows/${args.id}/${args.versionId}`);
            },
        },
        {
            name: 'n8n_update_workflow',
            description: 'Update a workflow by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                    name: { type: 'string', description: 'New name' },
                    nodes: { type: 'array', description: 'Updated nodes' },
                    connections: { type: 'object', description: 'Updated connections' },
                    settings: { type: 'object', description: 'Updated settings' },
                    staticData: { type: 'object', description: 'Updated static data' },
                    tags: { type: 'array', items: { type: 'string' }, description: 'Updated tag IDs' },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.put(`/workflows/${id}`, body);
            },
        },
        {
            name: 'n8n_delete_workflow',
            description: 'Delete a workflow by ID',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/workflows/${args.id}`);
            },
        },
        {
            name: 'n8n_activate_workflow',
            description: 'Activate a workflow',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/workflows/${args.id}/activate`);
            },
        },
        {
            name: 'n8n_deactivate_workflow',
            description: 'Deactivate a workflow',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/workflows/${args.id}/deactivate`);
            },
        },
        {
            name: 'n8n_archive_workflow',
            description: 'Archive a workflow',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/workflows/${args.id}/archive`);
            },
        },
        {
            name: 'n8n_unarchive_workflow',
            description: 'Unarchive a workflow',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.post(`/workflows/${args.id}/unarchive`);
            },
        },
        {
            name: 'n8n_transfer_workflow',
            description: 'Transfer a workflow to another project',
            inputSchema: {
                type: 'object',
                required: ['id', 'destinationProjectId'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                    destinationProjectId: { type: 'string', description: 'Target project ID' },
                },
            },
            execute: async (args) => {
                const { id, ...body } = args;
                return client.put(`/workflows/${id}/transfer`, body);
            },
        },
        {
            name: 'n8n_get_workflow_tags',
            description: 'Get tags for a workflow',
            inputSchema: {
                type: 'object',
                required: ['id'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/workflows/${args.id}/tags`);
            },
        },
        {
            name: 'n8n_set_workflow_tags',
            description: 'Set tags for a workflow',
            inputSchema: {
                type: 'object',
                required: ['id', 'tags'],
                properties: {
                    id: { type: 'string', description: 'Workflow ID' },
                    tags: {
                        type: 'array',
                        items: { type: 'object', properties: { id: { type: 'string' } } },
                        description: 'Array of tag objects with id field',
                    },
                },
            },
            execute: async (args) => {
                const { id, tags } = args;
                return client.put(`/workflows/${id}/tags`, tags);
            },
        },
    ];
}
