export type AnalysisMode = "overview" | "conflicts" | "quality" | "improvements";
export const analysisLabels: Record<AnalysisMode, string> = {
  overview: "解读概览", conflicts: "冲突检查", quality: "质量检查", improvements: "改进意见",
};
export type ReviewDocument = { name: string; source: string; content: string };

// Keep document boundaries and line references explicit, even for adversarial Markdown.
export function reviewInput(documents: ReviewDocument[]) {
  if (!documents.length || documents.length > 9) throw new Error("每次可检查当前文档及最多 8 份对照文档");
  for (const doc of documents) {
    if (!doc.content.trim()) throw new Error(`文档内容为空：${doc.name}`);
  }
  const text = JSON.stringify({ documents: documents.map((doc, index) => ({
    id: `D${index + 1}`, name: doc.name, source: doc.source,
    lines: doc.content.split(/\r?\n/).map((text, index) => ({ line: index + 1, text })),
  })) });
  if (new TextEncoder().encode(text).length > 256 * 1024) throw new Error("检查内容（含行号）超过 256 KiB，请减少对照文档或缩小文档后重试");
  return text;
}
