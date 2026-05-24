import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema, } from '@modelcontextprotocol/sdk/types.js';
import { N8nClient } from './client.js';
import { getAuditTools } from './tools/audit.js';
import { getCredentialTools } from './tools/credentials.js';
import { getExecutionTools } from './tools/executions.js';
import { getTagTools } from './tools/tags.js';
import { getWorkflowTools } from './tools/workflows.js';
import { getUserTools } from './tools/users.js';
import { getSourceControlTools } from './tools/sourceControl.js';
import { getVariableTools } from './tools/variables.js';
import { getDataTableTools } from './tools/dataTables.js';
import { getProjectTools } from './tools/projects.js';
import { getCommunityPackageTools } from './tools/communityPackages.js';
import { getMiscTools } from './tools/misc.js';
// Parse CLI args: --key=value or --key value
function parseArgs(argv) {
    const result = {};
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (arg.startsWith('--')) {
            const eqIdx = arg.indexOf('=');
            if (eqIdx !== -1) {
                const key = arg.slice(2, eqIdx);
                const value = arg.slice(eqIdx + 1);
                result[key] = value;
            }
            else {
                const key = arg.slice(2);
                const next = argv[i + 1];
                if (next && !next.startsWith('--')) {
                    result[key] = next;
                    i++;
                }
            }
        }
    }
    return result;
}
async function main() {
    const args = parseArgs(process.argv.slice(2));
    const baseUrl = args['baseUrl'] ?? process.env['N8N_BASE_URL'];
    const apiKey = args['apiKey'] ?? process.env['N8N_API_KEY'];
    if (!baseUrl) {
        console.error('Error: --baseUrl or N8N_BASE_URL environment variable is required');
        process.exit(1);
    }
    if (!apiKey) {
        console.error('Error: --apiKey or N8N_API_KEY environment variable is required');
        process.exit(1);
    }
    const client = new N8nClient({ baseUrl, apiKey });
    // Collect all tools
    const allTools = [
        ...getAuditTools(client),
        ...getCredentialTools(client),
        ...getExecutionTools(client),
        ...getTagTools(client),
        ...getWorkflowTools(client),
        ...getUserTools(client),
        ...getSourceControlTools(client),
        ...getVariableTools(client),
        ...getDataTableTools(client),
        ...getProjectTools(client),
        ...getCommunityPackageTools(client),
        ...getMiscTools(client),
    ];
    // Build lookup map
    const toolMap = new Map(allTools.map((t) => [t.name, t]));
    const server = new Server({ name: 'n8n-mcp-server', version: '1.0.0' }, { capabilities: { tools: {} } });
    server.setRequestHandler(ListToolsRequestSchema, async () => {
        return {
            tools: allTools.map(({ name, description, inputSchema }) => ({
                name,
                description,
                inputSchema,
            })),
        };
    });
    server.setRequestHandler(CallToolRequestSchema, async (request) => {
        const { name, arguments: toolArgs } = request.params;
        const tool = toolMap.get(name);
        if (!tool) {
            return {
                content: [{ type: 'text', text: `Unknown tool: ${name}` }],
                isError: true,
            };
        }
        try {
            const result = await tool.execute(toolArgs ?? {});
            return {
                content: [
                    {
                        type: 'text',
                        text: result === undefined ? 'Success (no content)' : JSON.stringify(result, null, 2),
                    },
                ],
            };
        }
        catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            return {
                content: [{ type: 'text', text: `Error: ${message}` }],
                isError: true,
            };
        }
    });
    const transport = new StdioServerTransport();
    await server.connect(transport);
    // Graceful shutdown
    process.on('SIGINT', async () => {
        await server.close();
        process.exit(0);
    });
    process.on('SIGTERM', async () => {
        await server.close();
        process.exit(0);
    });
}
main().catch((err) => {
    console.error('Fatal error:', err);
    process.exit(1);
});
