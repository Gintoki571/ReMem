
import { BaseToolHandler } from './BaseToolHandler.js';
import { formatToolResponse, formatToolError } from '@shared/index.js';
import type { ToolResponse } from '@shared/index.js';
import { getSqliteInstance } from '@infrastructure/database/index.js';

export class SqlToolHandler extends BaseToolHandler {
    async handleTool(name: string, args: Record<string, any>): Promise<ToolResponse> {
        try {
            this.validateArguments(args);

            if (name !== 'query_sql_db') {
                throw new Error(`Unknown operation: ${name}`);
            }

            const query = args.query.trim();

            // SAFETY CHECK: Only allow SELECT statements
            if (!/^SELECT\s/i.test(query)) {
                throw new Error("Security Alert: Only SELECT queries are allowed. INSERT, UPDATE, DELETE, DROP are strictly forbidden.");
            }

            // SAFETY CHECK: Block dangerous keywords even if starting with SELECT
            // (e.g. "SELECT * FROM nodes; DROP TABLE nodes")
            if (/;\s*(DROP|DELETE|UPDATE|INSERT|ALTER|TRUNCATE)/i.test(query)) {
                throw new Error("Security Alert: Multiple statements or dangerous keywords detected.");
            }

            const db = getSqliteInstance();

            // Execute the query
            // db.prepare().all() returns an array of row objects
            console.error(`[SqlTool] Executing: ${query}`);
            const stmt = db.prepare(query);
            const rows = stmt.all();

            return formatToolResponse({
                data: rows,
                actionTaken: `Executed SQL query: ${query}`,
                message: JSON.stringify(rows, null, 2)
            });

        } catch (error) {
            return formatToolError({
                operation: name,
                error: error instanceof Error ? error.message : 'Unknown error occurred',
                context: { args },
                suggestions: [
                    "Ensure your query starts with SELECT",
                    "Check table names: 'nodes', 'edges', 'embeddings'",
                    "This tool is READ-ONLY"
                ]
            });
        }
    }
}
