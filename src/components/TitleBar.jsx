import { useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Copy } from 'lucide-react';

export default function TitleBar() {
    const [isHovered, setIsHovered] = useState(false);
    const [isMaximized, setIsMaximized] = useState(false);

    const appWindow = getCurrentWindow();

    const handleMinimize = () => appWindow.minimize();
    const handleMaximize = async () => {
        const maximized = await appWindow.isMaximized();
        if (maximized) {
            await appWindow.unmaximize();
            setIsMaximized(false);
        } else {
            await appWindow.maximize();
            setIsMaximized(true);
        }
    };
    const handleClose = () => appWindow.close();

    return (
        <div
            data-tauri-drag-region
            className="h-8 bg-slate-900/80 backdrop-blur-sm flex items-center justify-between px-3 select-none shrink-0 border-b border-slate-800/50"
            onMouseEnter={() => setIsHovered(true)}
            onMouseLeave={() => setIsHovered(false)}
        >
            {/* App Title - Drag Region */}
            <div data-tauri-drag-region className="flex items-center gap-2 flex-1">
                <div className="w-4 h-4 rounded bg-gradient-to-br from-violet-500 to-purple-600" />
                <span className="text-xs font-semibold text-slate-400 tracking-wide">HipoCast</span>
            </div>

            {/* Window Controls - Show on hover */}
            <div
                className={`flex items-center gap-0.5 transition-opacity duration-200 ${
                    isHovered ? 'opacity-100' : 'opacity-0'
                }`}
            >
                <button
                    onClick={handleMinimize}
                    className="w-8 h-8 flex items-center justify-center text-slate-400 hover:bg-slate-700/50 hover:text-white transition-colors rounded"
                    title="Minimizar"
                >
                    <Minus className="w-3.5 h-3.5" />
                </button>
                <button
                    onClick={handleMaximize}
                    className="w-8 h-8 flex items-center justify-center text-slate-400 hover:bg-slate-700/50 hover:text-white transition-colors rounded"
                    title={isMaximized ? "Restaurar" : "Maximizar"}
                >
                    {isMaximized ? <Copy className="w-3 h-3" /> : <Square className="w-3 h-3" />}
                </button>
                <button
                    onClick={handleClose}
                    className="w-8 h-8 flex items-center justify-center text-slate-400 hover:bg-red-500 hover:text-white transition-colors rounded"
                    title="Fechar"
                >
                    <X className="w-3.5 h-3.5" />
                </button>
            </div>
        </div>
    );
}
