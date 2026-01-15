
import 'dotenv/config';

// Use exact values from .env
const url = process.env.OPENAI_BASE_URL + '/chat/completions'; // Append endpoint since base might not have /v1
// const urlWithV1 = process.env.OPENAI_BASE_URL.endsWith('/v1') ? process.env.OPENAI_BASE_URL : process.env.OPENAI_BASE_URL + '/v1';
const model = process.env.LLM_MODEL;

console.log('Testing connection with .env settings:');
console.log('URL:', url);
console.log('Model:', model);

async function test() {
    try {
        // Try the URL as configured + /chat/completions
        // Most clients expect base_url to be up to .../v1

        // Let's try to list models first to check base URL validity
        const modelsUrl = process.env.OPENAI_BASE_URL + '/v1/models'; // Try standard path
        console.log('Checking models at:', modelsUrl);

        try {
            const resp = await fetch(modelsUrl);
            if (resp.ok) {
                console.log('✅ Base URL seems (mostly) correct, found models endpoint at /v1/models');
                const data = await resp.json();
                console.log('Available models:', data.data.map((m: any) => m.id));
            } else {
                console.log('❌ Models check failed:', resp.status);
            }
        } catch (e) {
            console.log('❌ Models check failed (connection error)');

            // Try without /v1 in case user added it or something
            const modelsUrl2 = process.env.OPENAI_BASE_URL + '/models';
            console.log('Checking models at:', modelsUrl2);
            try {
                const resp2 = await fetch(modelsUrl2);
                if (resp2.ok) {
                    console.log('✅ Found models at /models');
                    const data = await resp2.json();
                    console.log('Available models:', data.data.map((m: any) => m.id));
                }
            } catch (e2) {
            console.error('[Test] Cleanup failed:', e2);
        }
        }

    } catch (error: any) {
        console.error('Error:', error.message);
    }
}

test();
