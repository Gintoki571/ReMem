
import { TokenEstimator } from '../core/tokenizer/TokenEstimator.js';

function testTokenEstimator() {
    console.log('[Test] Testing TokenEstimator...');

    const shortText = "Hello world";
    const longText = "This is a longer sentence that should have more tokens.";

    const tokenCountShort = TokenEstimator.countTokens(shortText);
    const tokenCountLong = TokenEstimator.countTokens(longText);

    console.log(`"${shortText}" -> ${tokenCountShort} tokens (Expected ~3)`);
    console.log(`"${longText}" -> ${tokenCountLong} tokens (Expected ~14)`);

    // Test truncation
    const limit = 5;
    const truncated = TokenEstimator.truncateToTokenLimit(longText, limit);
    console.log(`Truncating to ${limit} tokens: "${truncated}"`);

    if (tokenCountLong > tokenCountShort && truncated.length < longText.length) {
        console.log('SUCCESS: TokenEstimator logic seems sound.');
    } else {
        console.error('FAILURE: TokenEstimator logic check failed.');
        process.exit(1);
    }
}

testTokenEstimator();
