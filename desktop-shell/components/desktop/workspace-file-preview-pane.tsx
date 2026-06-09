"use client";

import dynamic from "next/dynamic";
import { Download, Presentation } from "lucide-react";
import { useEffect, useState } from "react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";

import { Button } from "@/components/ui/button";

const MonacoEditor = dynamic(() => import("@monaco-editor/react"), { ssr: false });
const PdfFilePreview = dynamic(() => import("@/components/pdf-file-preview"), { ssr: false });
const DocxFilePreview = dynamic(() => import("@/components/docx-file-preview"), { ssr: false });

const WORKSPACE_FILE_SCHEME = "workspace-file://";
const WORKSPACE_FILE_URL_PATTERN = /workspace-file:\/\/[^\s`<>()\[\]{}]+/g;
const WORKSPACE_FILE_PATTERN =
  /(?:\/[^\s`<>()\[\]{}]+?\.[A-Za-z0-9][^\s`<>()\[\]{}]*|(?:\.{1,2}\/|[A-Za-z0-9_.-]+\/)[^\s`<>()\[\]{}]+?\.[A-Za-z0-9][^\s`<>()\[\]{}]*)/g;
const WORKSPACE_FILE_PROTECTED_SEGMENT_PATTERN =
  /(```[\s\S]*?```|`[^`\n]+`|!?\[[^\]]*]\([^)]+\))/g;

export type WorkspaceFilePreview = {
  path: string;
  displayPath: string;
  fileName: string;
  fileType: string;
  mimeType: string;
  kind:
    | "text"
    | "markdown"
    | "image"
    | "pdf"
    | "audio"
    | "video"
    | "spreadsheet"
    | "document"
    | "presentation"
    | "binary";
  sizeBytes: number;
  content: string | null;
  sourceUrl: string | null;
  isBinary: boolean;
  truncated: boolean;
};

type SpreadsheetPreviewPayload = {
  type: "xlsx_preview";
  sheetCount: number;
  sheets: {
    name: string;
    rowCount: number;
    colCount: number;
    truncatedRows: boolean;
    truncatedCols: boolean;
    columns: string[];
    rows: {
      row: number;
      values: Array<string | number | boolean | null>;
    }[];
  }[];
};

type DocumentPreviewPayload = {
  type: "docx_preview";
  paragraphCount: number;
  truncated: boolean;
  paragraphs: string[];
};

type PptxPreviewPayload = {
  type: "pptx_preview";
  slideCount: number;
  truncated: boolean;
  slides: {
    id: string;
    title: string;
    bullets: string[];
  }[];
};

type PdfPreviewPayload = {
  type: "pdf_preview";
  pageCount: number;
  extractedPageCount: number;
  truncated: boolean;
  metadata: {
    title: string | null;
    author: string | null;
    subject: string | null;
    keywords: string[];
    creator: string | null;
  };
  pages: {
    id: string;
    pageNumber: number;
    label: string;
    charCount: number;
    truncated: boolean;
    text: string;
  }[];
};

function workspaceFileLanguage(fileType: string, fileName: string): string {
  const value = (fileType || fileName.split(".").pop() || "").toLowerCase();
  switch (value) {
    case "md":
    case "mdx":
      return "markdown";
    case "json":
      return "json";
    case "yml":
    case "yaml":
      return "yaml";
    case "toml":
      return "ini";
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
      return "javascript";
    case "rs":
      return "rust";
    case "py":
      return "python";
    case "css":
      return "css";
    case "html":
      return "html";
    case "xml":
      return "xml";
    case "sh":
      return "shell";
    case "sql":
      return "sql";
    default:
      return "plaintext";
  }
}

function parseSpreadsheetPreview(content: string | null): SpreadsheetPreviewPayload | null {
  if (!content) {
    return null;
  }
  try {
    const parsed = JSON.parse(content) as SpreadsheetPreviewPayload;
    return parsed?.type === "xlsx_preview" ? parsed : null;
  } catch {
    return null;
  }
}

function parseDocumentPreview(content: string | null): DocumentPreviewPayload | null {
  if (!content) {
    return null;
  }
  try {
    const parsed = JSON.parse(content) as DocumentPreviewPayload;
    return parsed?.type === "docx_preview" ? parsed : null;
  } catch {
    return null;
  }
}

function parsePptxPreview(content: string | null): PptxPreviewPayload | null {
  if (!content) {
    return null;
  }
  try {
    const parsed = JSON.parse(content) as PptxPreviewPayload;
    return parsed?.type === "pptx_preview" ? parsed : null;
  } catch {
    return null;
  }
}

function parsePdfPreview(content: string | null): PdfPreviewPayload | null {
  if (!content) {
    return null;
  }
  try {
    const parsed = JSON.parse(content) as PdfPreviewPayload;
    return parsed?.type === "pdf_preview" ? parsed : null;
  } catch {
    return null;
  }
}

function buildPdfExportText(preview: PdfPreviewPayload): string {
  const headerLines = [
    preview.metadata.title ? `Title: ${preview.metadata.title}` : null,
    preview.metadata.author ? `Author: ${preview.metadata.author}` : null,
    preview.metadata.subject ? `Subject: ${preview.metadata.subject}` : null,
    preview.metadata.keywords.length ? `Keywords: ${preview.metadata.keywords.join(", ")}` : null,
    `Pages extracted: ${preview.extractedPageCount}/${preview.pageCount}`,
  ].filter(Boolean);

  const body = preview.pages
    .map((page) => {
      const lines = [`Page ${page.pageNumber}`, page.text || "(no extracted text)"];
      return lines.join("\n");
    })
    .join("\n\n");

  return [...headerLines, "", body].join("\n").trim();
}

export function WorkspaceFilePreviewPane({
  file,
  loading,
  error,
  workspaceRoot,
  onOpenSlidevPreview,
  onExportSlidevDeck,
  onOpenMarkdownLink,
}: {
  file: WorkspaceFilePreview | null;
  loading: boolean;
  error: string | null;
  workspaceRoot?: string;
  onOpenSlidevPreview?: (file: WorkspaceFilePreview) => void;
  onExportSlidevDeck?: (file: WorkspaceFilePreview, format: "pdf" | "pptx") => void;
  onOpenMarkdownLink?: (file: WorkspaceFilePreview, href: string) => void;
}) {
  const [copiedPdfPath, setCopiedPdfPath] = useState<string | null>(null);

  useEffect(() => {
    setCopiedPdfPath(null);
  }, [file?.path]);

  if (loading) {
    return (
      <div className="flex h-full min-h-[320px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
        正在加载文件预览...
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-xl border border-rose-200 bg-rose-50/80 px-4 py-4 text-sm leading-6 text-rose-700">
        {error}
      </div>
    );
  }

  if (!file) {
    return (
      <div className="flex h-full min-h-[320px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 px-6 text-center text-sm leading-7 text-slate-500">
        点击对话里的文件路径，或从右侧文件树和最近列表选择一个文件来查看内容。
      </div>
    );
  }

  if (file.kind === "markdown" && file.content != null) {
    const markdown = linkifyWorkspacePathsInMarkdown(file.content, workspaceRoot);
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        {file.kind === "markdown" && (onOpenSlidevPreview || onExportSlidevDeck) ? (
          <div className="flex items-center justify-between gap-3 border-b border-slate-200 bg-slate-50/90 px-3 py-2">
            <div className="min-w-0 truncate text-xs text-slate-500">{file.displayPath}</div>
            <div className="flex shrink-0 items-center gap-2">
              {onExportSlidevDeck ? (
                <>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="h-7 shrink-0 gap-1.5 rounded-md px-2 text-xs"
                    onClick={() => onExportSlidevDeck(file, "pdf")}
                    title="使用隐藏窗口导出 PDF"
                  >
                    <Download className="size-3.5" />
                    PDF
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="h-7 shrink-0 gap-1.5 rounded-md px-2 text-xs"
                    onClick={() => onExportSlidevDeck(file, "pptx")}
                    title="实验性截图型 PPTX 导出"
                  >
                    <Download className="size-3.5" />
                    PPTX*
                  </Button>
                </>
              ) : null}
              {onOpenSlidevPreview ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  className="h-7 shrink-0 gap-1.5 rounded-md px-2 text-xs"
                  onClick={() => onOpenSlidevPreview(file)}
                  title="使用 Slidev 预览"
                >
                  <Presentation className="size-3.5" />
                  Slidev
                </Button>
              ) : null}
            </div>
          </div>
        ) : null}
        <div className="min-h-[320px] flex-1 overflow-auto px-5 py-4">
          <div className="prose prose-slate max-w-none text-[13px] leading-6 prose-headings:text-slate-900 prose-p:text-slate-700 prose-li:text-slate-700 prose-strong:text-slate-900 prose-code:text-[12px] prose-code:text-slate-800 prose-pre:overflow-x-auto prose-pre:rounded-xl prose-pre:border prose-pre:border-slate-200 prose-pre:bg-slate-50 prose-blockquote:border-slate-300 prose-blockquote:text-slate-600">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              urlTransform={markdownUrlTransform}
              components={{
                a: ({ href, children, ...props }) => {
                  if (href?.startsWith("#")) {
                    return (
                      <a
                        href={href}
                        className="text-sky-700 underline decoration-sky-300 underline-offset-2 hover:text-sky-800"
                        {...props}
                      >
                        {children}
                      </a>
                    );
                  }
                  return (
                    <a
                      href={href || "#"}
                      className="cursor-pointer text-sky-700 underline decoration-sky-300 underline-offset-2 hover:text-sky-800"
                      onClick={(event) => {
                        if (!href || !onOpenMarkdownLink) {
                          return;
                        }
                        event.preventDefault();
                        event.stopPropagation();
                        onOpenMarkdownLink(file, href);
                      }}
                      {...props}
                    >
                      {children}
                    </a>
                  );
                },
                code({ className, children, ...props }) {
                  const inline = !className;
                  if (inline) {
                    return (
                      <code className="rounded bg-slate-100 px-1.5 py-0.5 text-[12px] text-slate-800" {...props}>
                        {children}
                      </code>
                    );
                  }
                  return (
                    <code className={className} {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {markdown}
            </ReactMarkdown>
          </div>
        </div>
      </div>
    );
  }

  if (file.kind === "text" && file.content != null) {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="min-h-[320px] flex-1">
          <MonacoEditor
            height="100%"
            defaultLanguage={workspaceFileLanguage(file.fileType, file.fileName)}
            language={workspaceFileLanguage(file.fileType, file.fileName)}
            theme="vs"
            value={file.content}
            options={{
              readOnly: true,
              minimap: { enabled: false },
              scrollBeyondLastLine: false,
              wordWrap: "on",
              lineNumbersMinChars: 3,
              padding: { top: 16, bottom: 16 },
              fontSize: 13,
              lineHeight: 20,
              overviewRulerBorder: false,
              renderLineHighlight: "none",
              glyphMargin: false,
              folding: true,
              automaticLayout: true,
            }}
          />
        </div>
      </div>
    );
  }

  if (file.kind === "spreadsheet") {
    const preview = parseSpreadsheetPreview(file.content);
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="min-h-[320px] flex-1 overflow-auto px-3 py-3">
          {preview?.sheets.length ? (
            <div className="space-y-4">
              {preview.sheets.map((sheet) => (
                <section key={sheet.name} className="overflow-hidden rounded-xl border border-slate-200 bg-white">
                  <div className="border-b border-slate-200 px-4 py-3">
                    <div className="text-sm font-medium text-slate-900">{sheet.name}</div>
                    <div className="mt-1 text-xs text-slate-400">
                      {sheet.rowCount} 行 · {sheet.colCount} 列
                      {sheet.truncatedRows || sheet.truncatedCols ? " · 预览已裁剪" : ""}
                    </div>
                  </div>
                  <div className="overflow-auto">
                    <table className="min-w-full border-collapse text-left text-[12px] text-slate-700">
                      <thead className="bg-slate-50 text-slate-500">
                        <tr>
                          <th className="border-b border-r border-slate-200 px-3 py-2 font-medium">#</th>
                          {sheet.columns.map((column) => (
                            <th key={column} className="border-b border-slate-200 px-3 py-2 font-medium">
                              {column}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {sheet.rows.length ? (
                          sheet.rows.map((row) => (
                            <tr key={row.row} className="align-top">
                              <td className="border-r border-t border-slate-200 bg-slate-50/70 px-3 py-2 font-medium text-slate-500">
                                {row.row}
                              </td>
                              {row.values.map((value, index) => (
                                <td
                                  key={`${row.row}-${sheet.columns[index] || index}`}
                                  className="border-t border-slate-200 px-3 py-2"
                                >
                                  {value === null || value === "" ? (
                                    <span className="text-slate-300">·</span>
                                  ) : (
                                    String(value)
                                  )}
                                </td>
                              ))}
                            </tr>
                          ))
                        ) : (
                          <tr>
                            <td colSpan={sheet.columns.length + 1} className="px-3 py-6 text-center text-sm text-slate-400">
                              当前 sheet 没有可展示的数据。
                            </td>
                          </tr>
                        )}
                      </tbody>
                    </table>
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
              当前表格没有可展示的数据。
            </div>
          )}
        </div>
      </div>
    );
  }

  if (file.kind === "document") {
    const preview = parseDocumentPreview(file.content);
    if (file.sourceUrl) {
      return (
        <DocxFilePreview
          sourceUrl={file.sourceUrl}
          fallbackParagraphs={preview?.paragraphs || []}
          fallbackTruncated={preview?.truncated || false}
        />
      );
    }
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="min-h-[320px] flex-1 overflow-auto px-5 py-5">
          {preview?.paragraphs.length ? (
            <div className="space-y-4">
              {preview.paragraphs.map((paragraph, index) => (
                <div key={`${file.path}-p-${index}`} className="rounded-xl border border-slate-200 bg-white px-4 py-3">
                  <div className="mb-2 text-[11px] font-medium uppercase tracking-[0.08em] text-slate-400">
                    段落 {index + 1}
                  </div>
                  <p className="whitespace-pre-wrap break-words text-[14px] leading-7 text-slate-700">
                    {paragraph || <span className="text-slate-300">空段落</span>}
                  </p>
                </div>
              ))}
              {preview.truncated ? (
                <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50/70 px-4 py-3 text-sm text-slate-500">
                  预览只展示了前 {preview.paragraphs.length} 个段落。
                </div>
              ) : null}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
              当前文档没有可展示的段落内容。
            </div>
          )}
        </div>
      </div>
    );
  }

  if (file.kind === "presentation") {
    const preview = parsePptxPreview(file.content);
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="min-h-[320px] flex-1 overflow-auto px-5 py-5">
          {preview?.slides.length ? (
            <div className="space-y-4">
              {preview.slides.map((slide, index) => (
                <section
                  key={slide.id || `${file.path}-slide-${index}`}
                  className="overflow-hidden rounded-xl border border-slate-200 bg-white"
                >
                  <div className="border-b border-slate-200 bg-slate-50/80 px-4 py-3">
                    <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-slate-400">
                      Slide {index + 1}
                    </div>
                    <div className="mt-1 text-sm font-medium text-slate-900">{slide.title || "Untitled slide"}</div>
                  </div>
                  <div className="px-4 py-4">
                    {slide.bullets.length ? (
                      <ul className="space-y-2 text-[14px] leading-7 text-slate-700">
                        {slide.bullets.map((bullet, bulletIndex) => (
                          <li key={`${slide.id}-bullet-${bulletIndex}`} className="flex gap-3">
                            <span className="pt-[7px] text-slate-300">•</span>
                            <span className="min-w-0 flex-1 break-words whitespace-pre-wrap">{bullet}</span>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <div className="text-sm text-slate-400">当前幻灯片没有提取到正文要点。</div>
                    )}
                  </div>
                </section>
              ))}
              {preview.truncated ? (
                <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50/70 px-4 py-3 text-sm text-slate-500">
                  预览只展示了前 {preview.slides.length} 页幻灯片。
                </div>
              ) : null}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
              当前演示文稿没有可展示的幻灯片内容。
            </div>
          )}
        </div>
      </div>
    );
  }

  if (file.kind === "image" && file.sourceUrl) {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="flex min-h-[320px] flex-1 items-center justify-center bg-slate-50/70 p-4">
          <img
            src={file.sourceUrl}
            alt={file.fileName}
            className="max-h-full max-w-full rounded-lg object-contain shadow-[0_10px_30px_rgba(15,23,42,0.08)]"
          />
        </div>
      </div>
    );
  }

  if (file.kind === "pdf") {
    const preview = parsePdfPreview(file.content);
    const canCopyExtractedText = Boolean(preview?.pages.length);
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div
          className={
            preview?.pages.length
              ? "grid h-full min-h-0 flex-1 gap-3 p-3 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)]"
              : "h-full min-h-0 flex-1 p-3"
          }
        >
          <div className="h-full min-h-0 overflow-hidden rounded-xl border border-slate-200 bg-slate-50/70">
            {file.sourceUrl ? (
              <PdfFilePreview sourceUrl={file.sourceUrl} />
            ) : (
              <div className="flex h-full min-h-0 items-center justify-center px-6 text-center text-sm leading-7 text-slate-500">
                这个 PDF 体积较大，当前没有内嵌页面预览，但已保留可提取文本。
              </div>
            )}
          </div>
          {preview?.pages.length ? (
            <div className="h-full min-h-0 overflow-auto rounded-xl border border-slate-200 bg-white">
              <div className="border-b border-slate-200 px-4 py-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-sm font-medium text-slate-900">可提取文本</div>
                    <div className="mt-1 text-xs text-slate-400">
                      {preview.extractedPageCount} / {preview.pageCount} 页
                      {preview.truncated ? " · 已截断" : ""}
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={!canCopyExtractedText}
                    onClick={async () => {
                      if (!preview?.pages.length) {
                        return;
                      }
                      await navigator.clipboard.writeText(buildPdfExportText(preview));
                      setCopiedPdfPath(file.path);
                      window.setTimeout(() => {
                        setCopiedPdfPath((current) => (current === file.path ? null : current));
                      }, 1600);
                    }}
                    className="shrink-0"
                  >
                    {copiedPdfPath === file.path ? "已复制" : "复制文本"}
                  </Button>
                </div>
                {preview.metadata.title || preview.metadata.author || preview.metadata.subject ? (
                  <div className="mt-3 space-y-1 text-xs leading-6 text-slate-500">
                    {preview.metadata.title ? <div>标题：{preview.metadata.title}</div> : null}
                    {preview.metadata.author ? <div>作者：{preview.metadata.author}</div> : null}
                    {preview.metadata.subject ? <div>主题：{preview.metadata.subject}</div> : null}
                  </div>
                ) : null}
              </div>
              <div className="space-y-3 px-4 py-4">
                {preview.pages.map((page) => (
                  <section key={page.id} className="rounded-xl border border-slate-200 bg-slate-50/60 px-4 py-3">
                    <div className="mb-2 flex items-center justify-between gap-3 text-[11px] font-medium uppercase tracking-[0.08em] text-slate-400">
                      <span>Page {page.pageNumber}</span>
                      <span>
                        {page.charCount} chars
                        {page.truncated ? " · excerpt" : ""}
                      </span>
                    </div>
                    <p className="whitespace-pre-wrap break-words text-[13px] leading-6 text-slate-700">
                      {page.text || <span className="text-slate-300">当前页没有提取到文本。</span>}
                    </p>
                  </section>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    );
  }

  if (file.kind === "audio" && file.sourceUrl) {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="flex min-h-[320px] flex-1 items-center justify-center bg-slate-50/70 px-5">
          <audio controls src={file.sourceUrl} className="w-full max-w-[420px]" />
        </div>
      </div>
    );
  }

  if (file.kind === "video" && file.sourceUrl) {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
        <div className="flex min-h-[320px] flex-1 items-center justify-center bg-slate-50/70 p-4">
          <video controls src={file.sourceUrl} className="max-h-full max-w-full rounded-lg" />
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/88">
      <div className="min-h-[320px] flex-1 overflow-auto px-4 py-4">
        <p className="whitespace-pre-wrap break-words font-mono text-xs leading-6 text-slate-600">
          {file.content || "这个文件类型暂不支持内嵌预览。"}
        </p>
      </div>
    </div>
  );
}

function cleanWorkspaceFileToken(value: string): string {
  return value
    .replace(/^[([<{`'"]+/, "")
    .replace(/[)\]>}`'",;]+$/, "")
    .replace(/:(\d+)(?::\d+)?$/, "");
}

function normalizeWorkspaceFileReference(value: string, workspaceRoot?: string): string | null {
  const cleaned = cleanWorkspaceFileToken(value.trim());
  if (!cleaned || cleaned.includes("://")) {
    return null;
  }
  if (cleaned.startsWith("/")) {
    if (!workspaceRoot?.trim()) {
      return cleaned;
    }
    return cleaned.startsWith(workspaceRoot.trim()) ? cleaned : null;
  }
  if (cleaned.startsWith("./") || cleaned.startsWith("../")) {
    return cleaned;
  }
  if (cleaned.includes("/")) {
    return cleaned;
  }
  return null;
}

function workspaceFileHref(value: string): string {
  return `${WORKSPACE_FILE_SCHEME}${encodeURIComponent(value)}`;
}

function markdownUrlTransform(url: string): string {
  if (url.startsWith(WORKSPACE_FILE_SCHEME)) {
    return url;
  }
  return defaultUrlTransform(url);
}

function parseWorkspaceFileHref(value?: string | null): string | null {
  if (!value?.startsWith(WORKSPACE_FILE_SCHEME)) {
    return null;
  }
  try {
    return decodeURIComponent(value.slice(WORKSPACE_FILE_SCHEME.length));
  } catch {
    return null;
  }
}

function workspaceFileLabelFromHref(value: string): string {
  return parseWorkspaceFileHref(value) || value;
}

function linkifyWorkspacePathsInMarkdown(content: string, workspaceRoot?: string): string {
  return content
    .split(WORKSPACE_FILE_PROTECTED_SEGMENT_PATTERN)
    .map((segment) => {
      if (
        segment.startsWith("```") ||
        segment.startsWith("`") ||
        /^\!?\[[^\]]*]\([^)]+\)$/.test(segment)
      ) {
        return segment;
      }
      const withWorkspaceUrls = segment.replace(WORKSPACE_FILE_URL_PATTERN, (match) => {
        return `[${workspaceFileLabelFromHref(match)}](${match})`;
      });
      return withWorkspaceUrls.replace(WORKSPACE_FILE_PATTERN, (match) => {
        const resolved = normalizeWorkspaceFileReference(match, workspaceRoot);
        if (!resolved) {
          return match;
        }
        return `[${match}](${workspaceFileHref(resolved)})`;
      });
    })
    .join("");
}
