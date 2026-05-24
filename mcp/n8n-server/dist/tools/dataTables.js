export function getDataTableTools(client) {
    return [
        {
            name: 'n8n_list_data_tables',
            description: 'List all data tables',
            inputSchema: {
                type: 'object',
                properties: {
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                return client.get('/data-tables', args);
            },
        },
        {
            name: 'n8n_create_data_table',
            description: 'Create a new data table',
            inputSchema: {
                type: 'object',
                required: ['name'],
                properties: {
                    name: { type: 'string', description: 'Table name' },
                    columns: {
                        type: 'array',
                        items: {
                            type: 'object',
                            properties: {
                                name: { type: 'string' },
                                type: { type: 'string' },
                            },
                        },
                        description: 'Initial columns',
                    },
                },
            },
            execute: async (args) => {
                return client.post('/data-tables', args);
            },
        },
        {
            name: 'n8n_get_data_table',
            description: 'Get a data table by ID',
            inputSchema: {
                type: 'object',
                required: ['dataTableId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                },
            },
            execute: async (args) => {
                return client.get(`/data-tables/${args.dataTableId}`);
            },
        },
        {
            name: 'n8n_update_data_table',
            description: 'Update a data table by ID',
            inputSchema: {
                type: 'object',
                required: ['dataTableId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    name: { type: 'string', description: 'New table name' },
                },
            },
            execute: async (args) => {
                const { dataTableId, ...body } = args;
                return client.patch(`/data-tables/${dataTableId}`, body);
            },
        },
        {
            name: 'n8n_delete_data_table',
            description: 'Delete a data table by ID',
            inputSchema: {
                type: 'object',
                required: ['dataTableId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/data-tables/${args.dataTableId}`);
            },
        },
        {
            name: 'n8n_list_data_table_rows',
            description: 'List rows in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                const { dataTableId, ...query } = args;
                return client.get(`/data-tables/${dataTableId}/rows`, query);
            },
        },
        {
            name: 'n8n_create_data_table_rows',
            description: 'Create rows in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'rows'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    rows: {
                        type: 'array',
                        items: { type: 'object' },
                        description: 'Array of row objects to insert',
                    },
                },
            },
            execute: async (args) => {
                const { dataTableId, rows } = args;
                return client.post(`/data-tables/${dataTableId}/rows`, rows);
            },
        },
        {
            name: 'n8n_update_data_table_rows',
            description: 'Update rows in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'rows'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    rows: {
                        type: 'array',
                        items: { type: 'object' },
                        description: 'Array of row objects to update (must include row ID)',
                    },
                },
            },
            execute: async (args) => {
                const { dataTableId, rows } = args;
                return client.patch(`/data-tables/${dataTableId}/rows/update`, rows);
            },
        },
        {
            name: 'n8n_upsert_data_table_rows',
            description: 'Upsert rows in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'rows'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    rows: {
                        type: 'array',
                        items: { type: 'object' },
                        description: 'Array of row objects to upsert',
                    },
                },
            },
            execute: async (args) => {
                const { dataTableId, rows } = args;
                return client.post(`/data-tables/${dataTableId}/rows/upsert`, rows);
            },
        },
        {
            name: 'n8n_delete_data_table_rows',
            description: 'Delete rows from a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'rows'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    rows: {
                        type: 'array',
                        items: { type: 'object', properties: { id: { type: 'string' } } },
                        description: 'Array of row objects with id field to delete',
                    },
                },
            },
            execute: async (args) => {
                const { dataTableId, rows } = args;
                return client.request('DELETE', `/data-tables/${dataTableId}/rows/delete`, rows);
            },
        },
        {
            name: 'n8n_list_data_table_columns',
            description: 'List columns in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    limit: { type: 'number', description: 'Max results' },
                    cursor: { type: 'string', description: 'Pagination cursor' },
                },
            },
            execute: async (args) => {
                const { dataTableId, ...query } = args;
                return client.get(`/data-tables/${dataTableId}/columns`, query);
            },
        },
        {
            name: 'n8n_create_data_table_column',
            description: 'Create a column in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'name'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    name: { type: 'string', description: 'Column name' },
                    type: { type: 'string', description: 'Column type' },
                },
            },
            execute: async (args) => {
                const { dataTableId, ...body } = args;
                return client.post(`/data-tables/${dataTableId}/columns`, body);
            },
        },
        {
            name: 'n8n_update_data_table_column',
            description: 'Update a column in a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'columnId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    columnId: { type: 'string', description: 'Column ID' },
                    name: { type: 'string', description: 'New column name' },
                    type: { type: 'string', description: 'New column type' },
                },
            },
            execute: async (args) => {
                const { dataTableId, columnId, ...body } = args;
                return client.patch(`/data-tables/${dataTableId}/columns/${columnId}`, body);
            },
        },
        {
            name: 'n8n_delete_data_table_column',
            description: 'Delete a column from a data table',
            inputSchema: {
                type: 'object',
                required: ['dataTableId', 'columnId'],
                properties: {
                    dataTableId: { type: 'string', description: 'Data table ID' },
                    columnId: { type: 'string', description: 'Column ID' },
                },
            },
            execute: async (args) => {
                return client.delete(`/data-tables/${args.dataTableId}/columns/${args.columnId}`);
            },
        },
    ];
}
