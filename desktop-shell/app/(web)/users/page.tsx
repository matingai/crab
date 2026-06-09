import { Suspense } from "react";

import { UserDetailPageClient } from "@/components/web/user-detail-page-client";

export default function UserDetailPage() {
  return (
    <Suspense fallback={null}>
      <UserDetailPageClient />
    </Suspense>
  );
}
