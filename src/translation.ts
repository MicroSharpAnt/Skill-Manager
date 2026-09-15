import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import type { Root, Nodes, PhrasingContent } from "mdast";

export type TranslationStyle = "translation" | "columns" | "sentences";
export type Sentence = { id: number; start: number; end: number; original: string };
export type SentencePair = Sentence & { translated: string };
const parser = unified().use(remarkParse).use(remarkGfm);
export function documentParts(content: string) {
  const header = content.match(/^\uFEFF?---\r?\n[\s\S]*?\r?\n(?:---|\.\.\.)(?:\r?\n|$)/)?.[0] ?? "";
  return { header, body: content.slice(header.length) };
}
function walk(node: Nodes, visit: (node: Nodes) => boolean | void) {
  if (visit(node) === false) return;
  if ("children" in node) for (const child of node.children) walk(child, visit);
}
// Offsets always refer to the original Markdown body, never to translated text.
export function sentences(content: string): Sentence[] {
  const { body } = documentParts(content);
  const tree = parser.parse(body);
  const result: Sentence[] = [];
  const segmenter = new Intl.Segmenter("en", { granularity: "sentence" });
  walk(tree, node => {
    if (node.type !== "paragraph" && node.type !== "heading" && node.type !== "tableCell") return;
    const start = node.children[0]?.position?.start.offset;
    const end = node.children.at(-1)?.position?.end.offset;
    if (start == null || end == null) return false;
    const protectedRanges: [number, number][] = [];
    walk(node, child => {
      if (["inlineCode", "link", "linkReference", "image", "imageReference", "html"].includes(child.type)) {
        const a = child.position?.start.offset, b = child.position?.end.offset;
        if (a != null && b != null) protectedRanges.push([a, b]);
        return false;
      }
    });
    let cursor = start;
    for (const item of segmenter.segment(body.slice(start, end))) {
      const boundary = start + item.index + item.segment.length;
      if (protectedRanges.some(([a, b]) => a < boundary && boundary < b)) continue;
      const raw = body.slice(cursor, boundary);
      const leading = raw.length - raw.trimStart().length;
      const original = raw.trim();
      if (original && /\p{L}/u.test(original)) {
        // Pure code/image/HTML nodes do not need translating.
        const onlyProtected = node.children.every(c => ["inlineCode", "image", "imageReference", "html", "break"].includes(c.type) || (c.type === "text" && !c.value.trim()));
        if (!onlyProtected) result.push({ id: result.length, start: cursor + leading, end: cursor + leading + original.length, original });
      }
      cursor = boundary;
    }
    return false;
  });
  return result;
}
function protectedValues(content: string) {
  const values: string[] = [];
  walk(parser.parse(content), node => {
    if (node.type === "inlineCode" || node.type === "code" || node.type === "html") values.push(`${node.type}:${node.value}`);
    if (node.type === "link" || node.type === "image" || node.type === "definition") values.push(`url:${node.url}`);
  });
  return values;
}
export function combineTranslation(content: string, pairs: SentencePair[]): string {
  const { header, body } = documentParts(content);
  const expected = sentences(content);
  if (pairs.length !== expected.length) throw new Error("译文句数不完整，请重新翻译");
  let cursor = 0, output = header;
  for (let i = 0; i < expected.length; i++) {
    const original = expected[i], pair = pairs[i];
    if (pair.id !== original.id || pair.start !== original.start || pair.end !== original.end || pair.original !== original.original || !pair.translated.trim())
      throw new Error("原文与译文编号不匹配，请重新翻译");
    if (JSON.stringify(protectedValues(pair.original)) !== JSON.stringify(protectedValues(pair.translated)))
      throw new Error(`第 ${i + 1} 句译文改动了代码或链接，请重新翻译`);
    output += body.slice(cursor, original.start) + pair.translated;
    cursor = original.end;
  }
  return output + body.slice(cursor);
}
function inline(content: string): PhrasingContent[] {
  const node = parser.parse(content).children[0];
  return node && (node.type === "paragraph" || node.type === "heading") ? node.children : [{ type: "text", value: content }];
}
// Rendering plugin: keep original Markdown headings, lists, tables and code blocks.
// Only prose is replaced with paired display spans. Source files are untouched.
export function bilingualPlugin(content: string, pairs: SentencePair[]) {
  const { body } = documentParts(content);
  return function plugin() {
    return (tree: Root) => {
      walk(tree, node => {
        if (node.type !== "paragraph" && node.type !== "heading" && node.type !== "tableCell") return;
        const start = node.position?.start.offset, end = node.position?.end.offset;
        if (start == null || end == null) return false;
        const matches = pairs.filter(p => p.start >= start && p.end <= end);
        if (!matches.length) return false;
        let cursor = node.children[0]?.position?.start.offset ?? start;
        const children: PhrasingContent[] = [];
        for (const pair of matches) {
          const gap = body.slice(cursor, pair.start);
          if (gap.trim()) children.push(...inline(gap));
          children.push({
          type: "emphasis", data: { hName: "span", hProperties: { className: ["sentence-pair"] } },
          children: [
            { type: "emphasis", data: { hName: "span", hProperties: { className: ["sentence-original"] } }, children: inline(pair.original) },
            { type: "emphasis", data: { hName: "span", hProperties: { className: ["sentence-translated"] } }, children: inline(pair.translated) },
          ],
          });
          cursor = pair.end;
        }
        const tail = body.slice(cursor, node.children.at(-1)?.position?.end.offset ?? end);
        if (tail.trim()) children.push(...inline(tail));
        node.children = children;
        return false;
      });
    };
  };
}
