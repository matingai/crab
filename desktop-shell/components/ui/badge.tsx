import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-md px-3 py-1 text-[11px] font-medium tracking-[0.08em] uppercase",
  {
    variants: {
      variant: {
        default: "bg-slate-900 text-white",
        secondary: "bg-slate-100 text-slate-600",
        outline: "border border-slate-200 bg-white text-slate-600",
        soft: "bg-[rgba(74,108,247,0.1)] text-[rgb(74,108,247)]",
        success: "bg-emerald-50 text-emerald-700",
        danger: "bg-orange-50 text-orange-700"
      }
    },
    defaultVariants: {
      variant: "default"
    }
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof badgeVariants>) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
