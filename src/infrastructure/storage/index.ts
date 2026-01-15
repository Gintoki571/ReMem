// src/infrastructure/storage/index.ts

export { JsonLineStorage } from './JsonLineStorage.js';
export { SqliteStorage } from './SqliteStorage.js';
export type {
    IStorage,
    EdgeIndex
} from './IStorage.js';