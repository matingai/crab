"use client";

import { useEffect, useRef, useState } from "react";

type DocxFilePreviewProps = {
  sourceUrl: string;
  fallbackParagraphs: string[];
  fallbackTruncated: boolean;
};

export default function DocxFilePreview({
  sourceUrl,
  fallbackParagraphs,
  fallbackTruncated,
}: DocxFilePreviewProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const styleRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let observer: MutationObserver | null = null;

    async function renderDocx() {
      const bodyContainer = bodyRef.current;
      const styleContainer = styleRef.current;
      if (!bodyContainer || !styleContainer) {
        return;
      }

      bodyContainer.innerHTML = "";
      styleContainer.innerHTML = "";
      setStatus("loading");
      setErrorMessage(null);

      const updateRenderedPages = () => {
        if (cancelled) {
          return;
        }
        const pages = bodyContainer.querySelectorAll("section.docx-preview-shell").length;
        if (pages > 0) {
          setStatus("ready");
        } else if (bodyContainer.childElementCount > 0) {
          setStatus("ready");
        }
      };

      observer = new MutationObserver(updateRenderedPages);
      observer.observe(bodyContainer, { childList: true, subtree: true });

      try {
        const response = await fetch(sourceUrl);
        const blob = await response.blob();
        if (cancelled) {
          return;
        }

        const { renderAsync } = await import("docx-preview");
        if (cancelled) {
          return;
        }

        await renderAsync(blob, bodyContainer, styleContainer, {
          className: "docx-preview-shell",
          inWrapper: true,
          ignoreWidth: false,
          ignoreHeight: false,
          breakPages: true,
          ignoreLastRenderedPageBreak: false,
          ignoreFonts: false,
          renderHeaders: true,
          renderFooters: true,
          renderFootnotes: true,
          renderEndnotes: true,
        });

        if (!cancelled) {
          updateRenderedPages();
          setStatus("ready");
        }
      } catch (error) {
        if (!cancelled) {
          setStatus("error");
          setErrorMessage(error instanceof Error ? error.message : "未知错误");
        }
      }
    }

    void renderDocx();

    return () => {
      cancelled = true;
      observer?.disconnect();
      if (bodyRef.current) {
        bodyRef.current.innerHTML = "";
      }
      if (styleRef.current) {
        styleRef.current.innerHTML = "";
      }
    };
  }, [sourceUrl]);

  if (status === "error") {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-auto rounded-xl border border-slate-200 bg-white/88 px-5 py-5">
        <div className="rounded-xl border border-amber-200 bg-amber-50/80 px-4 py-3 text-sm leading-6 text-amber-800">
          Word 样式预览加载失败，已回退到文本段落预览。
          {errorMessage ? ` 原因：${errorMessage}` : ""}
        </div>
        <div className="mt-4 space-y-4">
          {fallbackParagraphs.length ? (
            fallbackParagraphs.map((paragraph, index) => (
              <div key={`fallback-${index}`} className="rounded-xl border border-slate-200 bg-white px-4 py-3">
                <div className="mb-2 text-[11px] font-medium uppercase tracking-[0.08em] text-slate-400">
                  段落 {index + 1}
                </div>
                <p className="whitespace-pre-wrap break-words text-[14px] leading-7 text-slate-700">
                  {paragraph || <span className="text-slate-300">空段落</span>}
                </p>
              </div>
            ))
          ) : (
            <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
              当前文档没有可展示的段落内容。
            </div>
          )}
          {fallbackTruncated ? (
            <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50/70 px-4 py-3 text-sm text-slate-500">
              预览只展示了前 {fallbackParagraphs.length} 个段落。
            </div>
          ) : null}
        </div>
      </div>
    );
  }

  return (
    <div className="relative h-full overflow-auto rounded-xl border border-slate-200 bg-[linear-gradient(180deg,rgba(241,245,249,0.95),rgba(226,232,240,0.95))]">
      {status === "loading" ? (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/70 text-sm text-slate-500 backdrop-blur-[1px]">
          正在渲染 Word 页面...
        </div>
      ) : null}
      <div ref={styleRef} />
      <div ref={bodyRef} className="docx-preview-root min-h-full" />
    </div>
  );
}
