
import { analyzer } from './src/application/services/Analyzer.ts';
import 'dotenv/config';

async function testAnaylzer() {
    console.log('Testing Analyzer with LM Studio...');
    console.log('Model:', process.env.LLM_MODEL);
    console.log('Base URL:', process.env.OPENAI_BASE_URL);

    const text = "The dragon Smaug guards a mountain of treasure in the Lonely Mountain. The dwarf king Thorin wants to reclaim his homeland from Smaug.";

    try {
        console.log('\nSending text to LLM...');
        const result = await analyzer.extractFromText(text);
        console.log('\n✅ Extraction Successful!');
        console.log(JSON.stringify(result, null, 2));
    } catch (error: any) {
        console.error('\n❌ Extraction Failed!');
        console.error('Error Message:', error.message);
        if (error.cause) console.error('Cause:', error.cause);
        if (error.response) console.error('Response:', await error.response.text());
        console.dir(error, { depth: 2 });
    }
}

testAnaylzer();
