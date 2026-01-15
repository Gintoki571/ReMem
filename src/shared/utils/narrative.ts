// src/shared/utils/narrative.ts

import type { Graph } from '@core/index.js';

/**
 * Converts a graph structure into a human-readable narrative.
 * Useful for LLMs to understand the context better.
 */
export function formatGraphAsNarrative(graph: Graph): string {
    if (!graph.nodes || graph.nodes.length === 0) {
        return "No relevant memories found in the knowledge graph.";
    }

    const nodeLines = graph.nodes.map(node => {
        let metadataStr = '';
        if (node.metadata && Object.keys(node.metadata).length > 0) {
            const metaEntries = Object.entries(node.metadata)
                .map(([k, v]) => `${k}: ${v}`)
                .join(', ');
            metadataStr = ` (Attributes: ${metaEntries})`;
        }
        return `- **${node.name}** [${node.nodeType}]${metadataStr}`;
    });

    const edgeLines = graph.edges.map(edge => {
        return `- **${edge.from}** --(${edge.edgeType})--> **${edge.to}**`;
    });

    let narrative = "### 🧠 Knowledge Graph Discovery\n\n";
    narrative += "**Found Entities:**\n" + nodeLines.join('\n');

    if (edgeLines.length > 0) {
        narrative += "\n\n**Relationships Observed:**\n" + edgeLines.join('\n');
    }

    return narrative;
}
