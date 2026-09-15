import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { bilingualPlugin, type SentencePair } from "./translation";
import "./markdown.css";

export function MarkdownPreview({
  content,
  className = "",
  label = "Markdown 内容预览",
  pairs,
}: {
  content: string;
  className?: string;
  label?: string;
  pairs?: SentencePair[];
}) {
  // Keep Skill metadata readable without interpreting YAML as Markdown.
  const frontmatter = content.match(/^\uFEFF?---\r?\n([\s\S]*?)\r?\n(?:---|\.\.\.)(?:\r?\n|$)/);
  const body = frontmatter ? content.slice(frontmatter[0].length) : content;

  return (
    <div className={`markdown-preview ${className}`} tabIndex={0} role="region" aria-label={label}>
      {frontmatter && (
        <details className="markdown-metadata">
          <summary>Skill 元信息</summary>
          <pre><code>{frontmatter[1]}</code></pre>
        </details>
      )}
      <Markdown
        remarkPlugins={pairs ? [remarkGfm, bilingualPlugin(content, pairs)] : [remarkGfm]}
        components={{
          a: ({ node, ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
          table: ({ node, ...props }) => <div className="markdown-table"><table {...props} /></div>,
        }}
      >
        {body}
      </Markdown>
    </div>
  );
}
