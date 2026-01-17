// src/core/logging/Logger.ts

/**
 * Structured Logger Service
 * 
 * Centralizes logging to ensure:
 * 1. Structured output (timestamps, levels, modules)
 * 2. Secret redaction (security)
 * 3. Configurable verbosity
 */

import { ENV } from '@config/env.js';

export enum LogLevel {
    DEBUG = 0,
    INFO = 1,
    WARN = 2,
    ERROR = 3,
}

export class Logger {
    // Current log level (default to INFO in production, DEBUG in dev)
    private static currentLevel: LogLevel = ENV.NODE_ENV === 'production' ? LogLevel.INFO : LogLevel.DEBUG;

    /**
     * Regex to catch potential API keys (sk-...)
     * Matches sk- followed by at least 20 alphanumeric/underscore/dash chars
     */
    private static SECRET_REGEX = /sk-[a-zA-Z0-9_\-]{20,}/g;

    /**
     * Redacts secrets from string or object
     */
    private static redact(message: any): any {
        if (typeof message === 'string') {
            return message.replace(this.SECRET_REGEX, '[REDACTED]');
        } else if (typeof message === 'object' && message !== null) {
            // Recursive redaction for objects
            try {
                const str = JSON.stringify(message);
                const redacted = str.replace(this.SECRET_REGEX, '[REDACTED]');
                return JSON.parse(redacted);
            } catch (e) {
                return message; // Circular reference or other error
            }
        }
        return message;
    }

    private static formatMessage(level: string, module: string, message: any, context?: any): string {
        const timestamp = new Date().toISOString();
        const safeMessage = this.redact(message);

        let log = `[${timestamp}] [${level}] [${module}] ${typeof safeMessage === 'string' ? safeMessage : JSON.stringify(safeMessage)}`;

        if (context) {
            log += ` ${JSON.stringify(this.redact(context))}`;
        }

        return log;
    }

    public static debug(module: string, message: any, context?: any): void {
        if (this.currentLevel <= LogLevel.DEBUG) {
            console.error(this.formatMessage('DEBUG', module, message, context));
        }
    }

    public static info(module: string, message: any, context?: any): void {
        if (this.currentLevel <= LogLevel.INFO) {
            console.error(this.formatMessage('INFO', module, message, context));
        }
    }

    public static warn(module: string, message: any, context?: any): void {
        if (this.currentLevel <= LogLevel.WARN) {
            console.error(this.formatMessage('WARN', module, message, context));
        }
    }

    public static error(module: string, message: any, error?: any): void {
        if (this.currentLevel <= LogLevel.ERROR) {
            let errorDetails = '';
            if (error instanceof Error) {
                errorDetails = ` Stack: ${error.stack}`;
            } else if (error) {
                errorDetails = ` Details: ${JSON.stringify(this.redact(error))}`;
            }

            console.error(this.formatMessage('ERROR', module, message) + errorDetails);
        }
    }
}
