import * as React from "react"
import { cva } from "class-variance-authority"
import { cn } from "@/lib/utils"

const badgeVariants = cva(
    "inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-semibold transition-colors",
    {
        variants: {
            variant: {
                default:
                    "border-transparent bg-violet-600 text-white",
                secondary:
                    "border-transparent bg-slate-700 text-slate-300",
                destructive:
                    "border-transparent bg-red-600 text-white",
                outline: "text-white border-slate-600",
                success:
                    "border-transparent bg-green-500/20 text-green-400",
                warning:
                    "border-transparent bg-yellow-500/20 text-yellow-400",
            },
        },
        defaultVariants: {
            variant: "default",
        },
    }
)

function Badge({ className, variant, ...props }) {
    return (
        <div className={cn(badgeVariants({ variant }), className)} {...props} />
    )
}

export { Badge, badgeVariants }
