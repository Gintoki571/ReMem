import { closeDatabase, getSqliteInstance } from "../src/infrastructure/database/index.js";

function migrate() {
  console.log("[Migration] Starting Schema V2 migration...");
  const db = getSqliteInstance();

  try {
    // 1. Migrate 'messages' table
    try {
      console.log("[Migration] Altering messages table...");
      db.exec(`ALTER TABLE messages ADD COLUMN is_summarized INTEGER DEFAULT 0`);
      console.log("[Migration] Added is_summarized to messages.");
    } catch (e: any) {
      if (e.message.includes("duplicate column name")) {
        console.log("[Migration] is_summarized already exists.");
      } else {
        throw e;
      }
    }

    // 2. Migrate 'global_state' table
    // Since sqlite ALTER COLUMN is limited, and this is just cache, we can recreate it.
    try {
      console.log("[Migration] Recreating global_state table...");
      db.exec(`DROP TABLE IF EXISTS global_state`);
      db.exec(`
                CREATE TABLE global_state (
                    key TEXT PRIMARY KEY,
                    content TEXT NOT NULL,
                    is_summarized INTEGER DEFAULT 0,
                    created_at INTEGER DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                )
            `);
      console.log("[Migration] global_state table output.");
    } catch (e) {
      console.error("[Migration] Failed to migrate global_state:", e);
    }

    console.log("[Migration] Success.");
  } catch (e) {
    console.error("[Migration] Failed:", e);
  } finally {
    closeDatabase();
  }
}

migrate();
