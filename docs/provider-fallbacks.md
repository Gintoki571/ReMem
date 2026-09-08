# Provider fallback candidates for RLM delegation

Date: 2026-09-08. Docs only, no code. Candidates from `~/.prime/agent/settings.json` recentModels.
Local `~/.prime/agent/models.json` covers only omniroute-* providers, so no local data on these six;
identities below come from OpenRouter model pages, vendor announcements, and OpenCode Zen docs.

Known failure background: opencode/* 401s at times, openrouter 402s under parallel load,
muse age-confirm friction, omniroute-gemini semaphore timeouts. All `:free` / Zen-free
targets inherit their own quota and availability risks.

| selector | what | context | risks |
|---|---|---|---|
| `openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` | NVIDIA Nemotron 3 Ultra, open-weight hybrid Mamba-MoE reasoning/orchestration model, 55B active / 550B total | 1,000,000 in, 65,536 max out (OpenRouter page) | `:free` strings: 50 req/day base, 1000/day only with >=10 credits bought, ~20 RPM; free prompts may be logged/used for training per provider policy; reddit reports weak coding output; 402s under parallel load observed on openrouter |
| `opencode/mimo-v2.5-free` | Xiaomi MiMo V2.5, native omnimodal (text/image/video) via OpenCode Zen, free promo | ~1,050,000 (OpenRouter xiaomi/mimo-v2.5 page; Zen alias assumed same) | Zen free-for-limited-time: can rotate or disappear without notice; opencode/* 401s seen at times; omnimodal != stronger code reasoning |
| `opencode/big-pickle` | OpenCode Zen stealth model, identity undisclosed, free for limited time | unconfirmed (no published figure; verify at probe time) | stealth = model behind alias can change silently; free promo can end; anecdotal coding-agent tuning, no benchmarks to trust |
| `openrouter/thinkingmachines/inkling-small:free` | Thinking Machines Lab Inkling Small, open-weight multimodal MoE, 12B active / 276B total | up to 1M per vendor family announcement; exact `:free` cap unconfirmed | `:free` quota/training-data strings as above; newer lab, endpoint maturity unproven under parallel delegation load |
| `openrouter/minimax/minimax-m3:free` | MiniMax M3, multimodal foundation model (text/image/video in, text out) | 1,048,576 (OpenRouter page) | `:free` quota/training-data strings as above; generalist multimodal, not a code specialist; 402s under parallel load observed on openrouter |
| `opencode/deepseek-v4-flash` | DeepSeek V4 Flash, 284B MoE (13B active), fast coding/agents variant, 1M context family | 1,000,000 in, 384K max out (OpenCode data + HF card); Zen alias assumed same | Zen availability/promo rotation; opencode/* 401s seen at times; flash = speed over max reasoning depth |

## Recommendation (probe first)

1. `opencode/deepseek-v4-flash` first: only candidate explicitly tuned for coding agents,
   1M context fits delegation payloads, and OpenCode usage rank suggests real capacity.
2. `openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` second: 1M context plus
   reasoning/orchestration design matches delegation work; open weights aid reproducibility.
Probe each with one sequential (non-parallel) task before any parallel fan-out, watch for
401/402/429, and keep omniroute-bai (zero-cost, working) as the safe default.
