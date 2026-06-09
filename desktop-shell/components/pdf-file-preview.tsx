"use client";

export default function PdfFilePreview({ sourceUrl }: { sourceUrl: string }) {
  const separator = sourceUrl.includes("#") ? "&" : "#";
  const viewerUrl = `${sourceUrl}${separator}toolbar=0&navpanes=0&scrollbar=1&view=FitH`;

  return (
    <div className="relative h-full overflow-hidden rounded-xl border border-slate-200 bg-slate-50/70">
      <iframe
        src={viewerUrl}
        title="PDF preview"
        className="h-full w-full border-0 bg-white"
      />
    </div>
  );
}
