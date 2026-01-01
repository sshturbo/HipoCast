import React from 'react';
import { motion, AnimatePresence } from 'framer-motion';

// Check if lib/utils exists? I'll check first. If not, I'll inline the helper.
import { clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs) {
    return twMerge(clsx(inputs))
}

export const AlertDialog = ({ children, open, onOpenChange }) => {
    return (
        <AnimatePresence>
            {open && (
                <div className="fixed inset-0 z-50 flex items-center justify-center">
                    {/* Backdrop */}
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 bg-black/80 backdrop-blur-sm"
                        onClick={() => onOpenChange?.(false)}
                    />
                    {/* Dialog */}
                    {children}
                </div>
            )}
        </AnimatePresence>
    );
};

export const AlertDialogContent = ({ children, className }) => (
    <motion.div
        initial={{ opacity: 0, scale: 0.95, y: 10 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.95, y: 10 }}
        transition={{ duration: 0.2 }}
        className={cn(
            "relative z-50 grid w-full max-w-lg gap-4 border border-slate-800 bg-slate-950 p-6 shadow-lg duration-200 sm:rounded-lg md:w-full",
            className
        )}
    >
        {children}
    </motion.div>
);

export const AlertDialogHeader = ({ className, ...props }) => (
    <div
        className={cn(
            "flex flex-col space-y-2 text-center sm:text-left",
            className
        )}
        {...props}
    />
);

export const AlertDialogFooter = ({ className, ...props }) => (
    <div
        className={cn(
            "flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2",
            className
        )}
        {...props}
    />
);

export const AlertDialogTitle = ({ className, ...props }) => (
    <h2
        className={cn(
            "text-lg font-semibold text-slate-50",
            className
        )}
        {...props}
    />
);

export const AlertDialogDescription = ({ className, ...props }) => (
    <p
        className={cn("text-sm text-slate-400", className)}
        {...props}
    />
);

export const AlertDialogAction = ({ className, onClick, ...props }) => (
    <button
        className={cn(
            "inline-flex h-10 items-center justify-center rounded-md bg-red-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-400 focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50",
            className
        )}
        onClick={onClick}
        {...props}
    />
);

export const AlertDialogCancel = ({ className, onClick, ...props }) => (
    <button
        className={cn(
            "mt-2 inline-flex h-10 items-center justify-center rounded-md border border-slate-800 bg-transparent px-4 py-2 text-sm font-semibold text-slate-300 transition-colors hover:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-slate-400 focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 sm:mt-0",
            className
        )}
        onClick={onClick}
        {...props}
    />
);
