export function getProjectTools(client) {
    return [
        {
            name: 'n8n_list_projects',
            description: 'List all projects',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                return client.get('/projects', args);
            },
        },
        {
            name: 'n8n_create_project',
            description: 'Create a new project',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Project name' },
                    type: { type: 'string', description: 'Project type' },
                },
            },
            execute: async (args) => {
                return client.post('/projects', args);
            },
        },
        {
            name: 'n8n_delete_project',
            description: 'Delete a project by ID',
            inputSchema: {
                type: 'object',
                required: ['projectId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/projects/${args.projectId}`);
            },
        },
        {
            name: 'n8n_update_project',
            description: 'Update a project by ID',
            inputSchema: {
                type: 'object',
                required: ['projectId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    name: { type: 'string', description: 'New project name' },
                },
            },
            execute: async (args) => {
                const { projectId, ...body } = args;
                return client.put(`/projects/${projectId}`, body);
            },
        },
        {
            name: 'n8n_list_project_users',
            description: 'List users in a project',
            inputSchema: {
                type: 'object',
                required: ['projectId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                const { projectId, ...query } = args;
                return client.get(`/projects/${projectId}/users`, query);
            },
        },
        {
            name: 'n8n_add_project_user',
            description: 'Add a user to a project',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'userId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    userId: { type: 'string', description: 'User ID' },
                    role: { type: 'string', description: 'User role in project' },
                },
            },
            execute: async (args) => {
                const { projectId, ...body } = args;
                return client.post(`/projects/${projectId}/users`, body);
            },
        },
        {
            name: 'n8n_remove_project_user',
            description: 'Remove a user from a project',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'userId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    userId: { type: 'string', description: 'User ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/projects/${args.projectId}/users/${args.userId}`);
            },
        },
        {
            name: 'n8n_update_project_user',
            description: 'Update a user\'s role in a project',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'userId', 'role'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    userId: { type: 'string', description: 'User ID' },
                    role: { type: 'string', description: 'New role in project' },
                },
            },
            execute: async (args) => {
                const { projectId, userId, role } = args;
                const body = { role: role };
                return client.patch(`/projects/${projectId}/users/${userId}`, body);
            },
        },
    ];
}
