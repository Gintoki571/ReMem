import { existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

export const MODELS_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "models");
export const MODEL_AVAILABLE = existsSync(join(MODELS_DIR, "onnx", "model.onnx"));
