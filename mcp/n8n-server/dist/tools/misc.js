export function getMiscTools(client) {
    return [
        // Discover
        {
            name: 'n8n_discover',
            description: 'Discover available n8n API endpoints and capabilities',
            inputSchema: {
                type: 'object',
                properties: {},
            },
            execute: async (_args) => {
                return client.get('/discover');
            },
        },
        // Insights
        {
            name: 'n8n_get_insights_summary',
            description: 'Get insights summary for the n8n instance',
            inputSchema: {
                type: 'object',
                properties: {},
            },
            execute: async (_args) => {
                return client.get('/insights/summary');
            },
        },
        // Folders
        {
            name: 'n8n_list_project_folders',
            description: 'List folders in a project',
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
                return client.get(`/projects/${projectId}/folders`, query);
            },
        },
        {
            name: 'n8n_create_project_folder',
            description: 'Create a folder in a project',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'name'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    name: { type: 'string', description: 'Folder name' },
                    parentFolderId: { type: 'string', description: 'Parent folder ID (for nested folders)' },
                },
            },
            execute: async (args) => {
                const { projectId, ...body } = args;
                return client.post(`/projects/${projectId}/folders`, body);
            },
        },
        {
            name: 'n8n_get_project_folder',
            description: 'Get a folder by ID',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'folderId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    folderId: { type: 'string', description: 'Folder ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/projects/${args.projectId}/folders/${args.folderId}`);
            },
        },
        {
            name: 'n8n_update_project_folder',
            description: 'Update a folder',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'folderId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    folderId: { type: 'string', description: 'Folder ID' },
                    name: { type: 'string', description: 'New folder name' },
                    parentFolderId: { type: 'string', description: 'New parent folder ID' },
                },
            },
            execute: async (args) => {
                const { projectId, folderId, ...body } = args;
                return client.patch(`/projects/${projectId}/folders/${folderId}`, body);
            },
        },
        {
            name: 'n8n_delete_project_folder',
            description: 'Delete a folder',
            inputSchema: {
                type: 'object',
                required: ['projectId', 'folderId'],
                properties: {
                    projectId: { type: 'string', description: 'Project ID' },
                    folderId: { type: 'string', description: 'Folder ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/projects/${args.projectId}/folders/${args.folderId}`);
            },
        },
    ];
}
