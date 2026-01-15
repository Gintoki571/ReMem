export class RememError extends Error {
    constructor(
        message: string,
        public code: string,
        public details?: Record<string, any>,
        public suggestion?: string
    ) {
        super(message);
        this.name = 'RememError';
        Error.captureStackTrace(this, this.constructor);
    }

    toJSON() {
        return {
            name: this.name,
            message: this.message,
            code: this.code,
            details: this.details,
            suggestion: this.suggestion,
            stack: this.stack,
        };
    }
}

export class ValidationError extends RememError {
    constructor(message: string, field?: string, details?: Record<string, any>) {
        super(
            message,
            'VALIDATION_ERROR',
            { field, ...details },
            'Check the input parameters and try again'
        );
        this.name = 'ValidationError';
    }
}

export class StorageError extends RememError {
    constructor(message: string, operation?: string, details?: Record<string, any>) {
        super(
            message,
            'STORAGE_ERROR',
            { operation, ...details },
            'Check file permissions and disk space'
        );
        this.name = 'StorageError';
    }
}

export class DatabaseError extends RememError {
    constructor(message: string, query?: string, details?: Record<string, any>) {
        super(
            message,
            'DATABASE_ERROR',
            { query, ...details },
            'Check database connection and schema'
        );
        this.name = 'DatabaseError';
    }
}

export class VectorStoreError extends RememError {
    constructor(message: string, operation?: string, details?: Record<string, any>) {
        super(
            message,
            'VECTOR_STORE_ERROR',
            { operation, ...details },
            'Check LanceDB connection and configuration'
        );
        this.name = 'VectorStoreError';
    }
}

export class LLMError extends RememError {
    constructor(message: string, model?: string, details?: Record<string, any>) {
        super(
            message,
            'LLM_ERROR',
            { model, ...details },
            'Check API key, model availability, and rate limits'
        );
        this.name = 'LLMError';
    }
}

export class GraphError extends RememError {
    constructor(message: string, operation?: string, details?: Record<string, any>) {
        super(
            message,
            'GRAPH_ERROR',
            { operation, ...details },
            'Check node/edge names and relationships'
        );
        this.name = 'GraphError';
    }
}

export class ModuleError extends RememError {
    constructor(message: string, moduleName?: string, details?: Record<string, any>) {
        super(
            message,
            'MODULE_ERROR',
            { moduleName, ...details },
            'Check module configuration and availability'
        );
        this.name = 'ModuleError';
    }
}

export class ToolError extends RememError {
    constructor(message: string, toolName?: string, details?: Record<string, any>) {
        super(
            message,
            'TOOL_ERROR',
            { toolName, ...details },
            'Check tool parameters and availability'
        );
        this.name = 'ToolError';
    }
}

export function isRememError(error: unknown): error is RememError {
    return error instanceof RememError;
}

export function formatError(error: unknown): { message: string; code: string; details?: Record<string, any> } {
    if (isRememError(error)) {
        return {
            message: error.message,
            code: error.code,
            details: error.details,
        };
    }

    if (error instanceof Error) {
        return {
            message: error.message,
            code: 'UNKNOWN_ERROR',
            details: { stack: error.stack },
        };
    }

    return {
        message: String(error),
        code: 'UNKNOWN_ERROR',
    };
}
