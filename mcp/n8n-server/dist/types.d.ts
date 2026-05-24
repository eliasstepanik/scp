export interface N8nConfig {
    baseUrl: string;
    apiKey: string;
}
export interface PaginationParams {
    limit?: number;
    cursor?: string;
}
export interface WorkflowListParams extends PaginationParams {
    active?: boolean;
    tags?: string;
    name?: string;
    projectId?: string;
    excludePinnedData?: boolean;
    onlyActive?: boolean;
}
export interface WorkflowNode {
    id: string;
    name: string;
    type: string;
    typeVersion: number;
    position: [number, number];
    parameters?: Record<string, unknown>;
    credentials?: Record<string, unknown>;
}
export interface WorkflowConnection {
    [key: string]: {
        main?: Array<Array<{
            node: string;
            type: string;
            index: number;
        }>>;
    };
}
export interface WorkflowSettings {
    executionOrder?: string;
    saveManualExecutions?: boolean;
    callerPolicy?: string;
    errorWorkflow?: string;
    timezone?: string;
}
export interface WorkflowCreateBody {
    name: string;
    nodes: WorkflowNode[];
    connections: WorkflowConnection;
    settings?: WorkflowSettings;
    staticData?: Record<string, unknown>;
    tags?: string[];
    projectId?: string;
}
export interface WorkflowUpdateBody {
    name?: string;
    nodes?: WorkflowNode[];
    connections?: WorkflowConnection;
    settings?: WorkflowSettings;
    staticData?: Record<string, unknown>;
    tags?: string[];
}
export interface CredentialListParams extends PaginationParams {
    includeData?: boolean;
    type?: string;
    name?: string;
    projectId?: string;
}
export interface CredentialCreateBody {
    name: string;
    type: string;
    data: Record<string, unknown>;
    projectId?: string;
}
export interface CredentialUpdateBody {
    name?: string;
    data?: Record<string, unknown>;
}
export interface ExecutionListParams extends PaginationParams {
    workflowId?: string;
    status?: 'error' | 'success' | 'waiting' | 'running' | 'canceled';
    includeData?: boolean;
    projectId?: string;
}
export interface TagCreateBody {
    name: string;
}
export interface TagUpdateBody {
    name: string;
}
export interface UserListParams extends PaginationParams {
    includeRole?: boolean;
    projectId?: string;
}
export interface UserCreateBody {
    email: string;
    role?: string;
    firstName?: string;
    lastName?: string;
}
export interface UserRoleUpdateBody {
    newRoleName: string;
}
export interface VariableCreateBody {
    key: string;
    value: string;
    type?: string;
}
export interface VariableUpdateBody {
    key?: string;
    value?: string;
    type?: string;
}
export interface DataTableCreateBody {
    name: string;
    columns?: Array<{
        name: string;
        type?: string;
    }>;
}
export interface DataTableUpdateBody {
    name?: string;
}
export interface DataTableRowsParams extends PaginationParams {
    [key: string]: unknown;
}
export interface DataTableColumnCreateBody {
    name: string;
    type?: string;
}
export interface DataTableColumnUpdateBody {
    name?: string;
    type?: string;
}
export interface ProjectCreateBody {
    name: string;
    type?: string;
}
export interface ProjectUpdateBody {
    name?: string;
}
export interface ProjectUserAddBody {
    userId: string;
    role?: string;
}
export interface ProjectUserUpdateBody {
    role: string;
}
export interface CommunityPackageInstallBody {
    name: string;
}
export interface CommunityPackageUpdateBody {
    name: string;
}
export interface SourceControlPullBody {
    force?: boolean;
    variables?: Record<string, string>;
}
export interface AuditGenerateBody {
    additionalOptions?: {
        daysAbandonedWorkflow?: number;
        categories?: string[];
    };
}
export interface FolderCreateBody {
    name: string;
    parentFolderId?: string;
}
export interface FolderUpdateBody {
    name?: string;
    parentFolderId?: string;
}
export interface ApiListResponse<T> {
    data: T[];
    nextCursor?: string;
}
