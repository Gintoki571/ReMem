
const url = 'http://localhost:1234/v1/models';
// const model = 'gemma-3-27b-abliterated-i1/gemma-3-27b-abliterated.i1-IQ3_XXS.gguf';

console.log('Testing direct connection to:', url);

async function test() {
    try {
        const response = await fetch(url, {
            method: 'GET', // Changed to GET
            headers: {
                // 'Content-Type': 'application/json', // Not needed for GET
                'Authorization': 'Bearer lm-studio'
            },
            // body: JSON.stringify({...}) // Remove body
        });

        console.log('Status:', response.status, response.statusText);
        const text = await response.text();
        console.log('Body:', text);

    } catch (error) {
        console.error('Fetch Error:', error);
    }
}

test();
