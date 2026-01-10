// tests/verify_librarian.ts

import { librarianService } from '../application/services/LibrarianService.js';
import { CONFIG } from '../config/config.js';

async function runTest() {
    console.log("--- Librarian Verification ---");

    const testContexts = [
        {
            text: "I want to create a new NPC named Elara who is a blacksmith.",
            expected: "rpg"
        },
        {
            text: "How do I implement a singleton pattern in Typescript?",
            expected: "coding"
        },
        {
            text: "The player character visits a location called Frosthaven.",
            expected: "rpg"
        },
        {
            text: "I need to refactor the database connection logic in the engine.",
            expected: "coding"
        }
    ];

    for (const test of testContexts) {
        console.log(`\nAnalyzing Context: "${test.text}"`);
        const suggestions = await librarianService.suggestModules(test.text);

        if (suggestions.length > 0) {
            const topMatch = suggestions[0];
            const pass = topMatch.name === test.expected;
            console.log(`${pass ? '✅ SUCCESS' : '❌ FAILURE'}: Suggested '${topMatch.name}' (Score: ${topMatch.score})`);
            console.log(`Reason: ${topMatch.reason}`);
        } else {
            console.log("❌ FAILURE: No suggestions found.");
        }
    }
}

runTest().catch(console.error);
