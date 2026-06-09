import type { Metadata } from "next";

import "./globals.css";

export const metadata: Metadata = {
  title: "Crab",
  description: "Desktop shell for Crab",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-CN">
      <body>{children}</body>
    </html>
  );
}
