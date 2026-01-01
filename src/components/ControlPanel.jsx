import { useState, useEffect } from 'react';
import { fetchWindows, startStream, startBrowserStream } from '@/api/streams';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Combobox } from '@/components/ui/combobox';
import { Badge } from '@/components/ui/badge';
import { Loader2, Play, RefreshCw, Monitor, Zap, Layout, Globe } from 'lucide-react';
import { cn } from '@/lib/utils';
import { motion, AnimatePresence } from 'framer-motion';

export default function ControlPanel({ onStreamStarted }) {
    const [mode, setMode] = useState('window'); // 'window' ou 'link'
    const [windows, setWindows] = useState([]);
    const [selectedWindow, setSelectedWindow] = useState('');
    const [browserUrl, setBrowserUrl] = useState('');
    const [streamId, setStreamId] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');

    useEffect(() => {
        loadWindows();
    }, []);

    const loadWindows = async () => {
        const windowList = await fetchWindows();
        setWindows(windowList);
        if (windowList.length > 0) {
            setSelectedWindow(JSON.stringify(windowList[0]));
        }
    };

    const handleStart = async () => {
        if (mode === 'window' && !selectedWindow) {
            setError('Selecione uma fonte de captura');
            return;
        }
        if (mode === 'link' && !browserUrl) {
            setError('Insira uma URL válida');
            return;
        }

        setLoading(true);
        setError('');

        try {
            if (mode === 'window') {
                const win = JSON.parse(selectedWindow);
                const finalTitle = streamId.trim() || win.title || 'Stream';
                
                // Se tem streamId customizado, usar ele como ID real
                // Senão, usar o ID da janela
                const finalId = streamId.trim() || win.id;
                await startStream(finalId, finalTitle);
            } else {
                // Para browser stream, enviar customId separado
                const finalTitle = streamId.trim() || 'Web Stream';
                if (streamId.trim()) {
                    const customStreamId = streamId.trim();
                    await startBrowserStream(browserUrl, finalTitle, customStreamId);
                } else {
                    await startBrowserStream(browserUrl, finalTitle);
                }
            }

            onStreamStarted?.();
            setStreamId('');
            setBrowserUrl('');
        } catch (err) {
            setError(err.message || 'Erro ao iniciar stream');
        } finally {
            setLoading(false);
        }
    };

    return (
        <div className="relative group">
            {/* Background Glow Effect */}
            <div className="absolute -inset-1 bg-gradient-to-r from-violet-600/20 to-indigo-600/20 rounded-3xl blur-2xl opacity-50 group-hover:opacity-100 transition duration-1000 group-hover:duration-200"></div>

            <Card className="relative border-slate-800/50 bg-slate-900/50 backdrop-blur-xl overflow-hidden rounded-2xl shadow-2xl">
                <CardContent className="p-0">
                    <div className="flex flex-col">
                        {/* Header Section */}
                        <div className="px-8 py-6 border-b border-slate-800/50 flex items-center justify-between bg-gradient-to-r from-slate-900/80 to-slate-900/40">
                            <div className="flex items-center gap-3">
                                <div className="p-2 rounded-xl bg-violet-500/10 border border-violet-500/20">
                                    <Layout className="w-5 h-5 text-violet-400" />
                                </div>
                                <div>
                                    <h3 className="text-base font-bold text-white tracking-wide uppercase">Configurador de Transmissão</h3>
                                    <p className="text-[11px] text-slate-500 font-medium tracking-wider uppercase">Configure sua fonte e inicie o pipeline</p>
                                </div>
                            </div>
                            <div className="flex items-center gap-2">
                                <Badge variant="outline" className="bg-slate-950/50 border-slate-800 text-[10px] text-slate-400 py-1 px-3 uppercase tracking-tighter">
                                    Status: Ready
                                </Badge>
                            </div>
                        </div>

                        {/* Controls Content */}
                        <div className="p-8 space-y-8">
                            <div className="flex gap-4 p-1 bg-slate-950/50 border border-slate-800/50 rounded-2xl w-fit">
                                <button
                                    onClick={() => setMode('window')}
                                    className={cn(
                                        "flex items-center gap-2 px-6 py-2.5 rounded-xl text-[10px] font-black uppercase tracking-widest transition-all",
                                        mode === 'window'
                                            ? "bg-violet-600 text-white shadow-lg shadow-violet-600/20"
                                            : "text-slate-500 hover:text-slate-300"
                                    )}
                                >
                                    <Monitor className="w-3.5 h-3.5" />
                                    Captura de Janela
                                </button>
                                <button
                                    onClick={() => setMode('link')}
                                    className={cn(
                                        "flex items-center gap-2 px-6 py-2.5 rounded-xl text-[10px] font-black uppercase tracking-widest transition-all",
                                        mode === 'link'
                                            ? "bg-violet-600 text-white shadow-lg shadow-violet-600/20"
                                            : "text-slate-500 hover:text-slate-300"
                                    )}
                                >
                                    <Globe className="w-3.5 h-3.5" />
                                    Link da Web
                                </button>
                            </div>

                            <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-end">
                                {/* Fonte de Captura */}
                                <div className="lg:col-span-8 space-y-3">
                                    <AnimatePresence mode="wait">
                                        {mode === 'window' ? (
                                            <motion.div
                                                key="window"
                                                initial={{ opacity: 0, x: -10 }}
                                                animate={{ opacity: 1, x: 0 }}
                                                exit={{ opacity: 0, x: 10 }}
                                                className="space-y-3"
                                            >
                                                <div className="flex items-center justify-between px-1">
                                                    <label className="text-[10px] font-black text-slate-400 uppercase tracking-[0.2em] flex items-center gap-2">
                                                        <Monitor className="w-3 h-3 text-violet-500" />
                                                        Fonte de Captura Primária
                                                    </label>
                                                    <button
                                                        onClick={loadWindows}
                                                        disabled={loading}
                                                        className="text-[10px] font-bold text-violet-400 hover:text-violet-300 transition-colors flex items-center gap-1.5 uppercase tracking-widest"
                                                    >
                                                        <RefreshCw className={cn("w-3 h-3", loading && "animate-spin")} />
                                                        Recarregar Lista
                                                    </button>
                                                </div>
                                                <div className="relative">
                                                    <Combobox
                                                        value={selectedWindow}
                                                        onChange={setSelectedWindow}
                                                        disabled={loading}
                                                        placeholder="Selecione para onde apontar a câmera..."
                                                        options={windows.map((win, idx) => ({
                                                            value: JSON.stringify(win),
                                                            label: `${win.type === 'desktop' ? '🖥️ ' : '🪟 '} ${win.title}`
                                                        }))}
                                                    />
                                                </div>
                                            </motion.div>
                                        ) : (
                                            <motion.div
                                                key="link"
                                                initial={{ opacity: 0, x: 10 }}
                                                animate={{ opacity: 1, x: 0 }}
                                                exit={{ opacity: 0, x: -10 }}
                                                className="space-y-3"
                                            >
                                                <div className="px-1">
                                                    <label className="text-[10px] font-black text-slate-400 uppercase tracking-[0.2em] flex items-center gap-2">
                                                        <Globe className="w-3 h-3 text-violet-500" />
                                                        URL do Site (Youtube, Dashboards, etc)
                                                    </label>
                                                </div>
                                                <div className="relative">
                                                    <Input
                                                        placeholder="https://www.youtube.com/watch?v=..."
                                                        value={browserUrl}
                                                        onChange={(e) => setBrowserUrl(e.target.value)}
                                                        disabled={loading}
                                                        className="h-11 bg-slate-900/20 backdrop-blur-md border-slate-700/50 hover:border-slate-600 focus:ring-2 focus:ring-violet-500/30 transition-all text-sm rounded-xl pl-4"
                                                    />
                                                </div>
                                            </motion.div>
                                        )}
                                    </AnimatePresence>
                                </div>

                                {/* Config e Botão */}
                                <div className="lg:col-span-7 space-y-3">
                                    <div className="px-1">
                                        <label className="text-[10px] font-black text-slate-400 uppercase tracking-[0.2em] flex items-center gap-2">
                                            <Zap className="w-3 h-3 text-violet-500" />
                                            Identificador de Link (Opcional)
                                        </label>
                                    </div>
                                    <Input
                                        placeholder="ex: live_gaming_01"
                                        value={streamId}
                                        onChange={(e) => setStreamId(e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, ''))}
                                        disabled={loading}
                                        className="bg-slate-950/50 border-slate-800/80 h-14 text-sm focus:ring-2 focus:ring-violet-500/20 rounded-xl"
                                    />
                                </div>

                                <div className="lg:col-span-5">
                                    <Button
                                        onClick={handleStart}
                                        disabled={loading || (mode === 'window' ? !selectedWindow : !browserUrl)}
                                        className={cn(
                                            "w-full h-14 font-black uppercase tracking-[0.15em] text-xs transition-all duration-300 rounded-xl",
                                            loading
                                                ? "bg-slate-800 text-slate-500 cursor-not-allowed"
                                                : "bg-gradient-to-r from-violet-600 to-indigo-600 hover:from-violet-500 hover:to-indigo-500 text-white shadow-lg shadow-violet-600/20 hover:shadow-violet-600/40 transform hover:-translate-y-0.5 active:translate-y-0"
                                        )}
                                    >
                                        {loading ? (
                                            <Loader2 className="w-5 h-5 animate-spin mr-3" />
                                        ) : (
                                            <Play className="w-4 h-4 mr-3 fill-current" />
                                        )}
                                        {loading ? 'Inicializando Engine...' : mode === 'link' ? 'Abrir Link e Stream' : 'Abrir Transmissão'}
                                    </Button>
                                </div>
                            </div>

                            {/* Error Message */}
                            <AnimatePresence>
                                {error && (
                                    <motion.div
                                        initial={{ opacity: 0, height: 0 }}
                                        animate={{ opacity: 1, height: 'auto' }}
                                        exit={{ opacity: 0, height: 0 }}
                                        className="pt-2"
                                    >
                                        <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-red-400 text-xs font-bold uppercase tracking-wider">
                                            <Zap className="w-3 h-3 rotate-180" />
                                            {error}
                                        </div>
                                    </motion.div>
                                )}
                            </AnimatePresence>
                        </div>
                    </div>
                </CardContent>
            </Card>
        </div>
    );
}
