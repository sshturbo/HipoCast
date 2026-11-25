import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import Sidebar from '@/components/Sidebar';
import ControlPanel from '@/components/ControlPanel';
import StreamGrid from '@/components/StreamGrid';
import SettingsPage from '@/components/SettingsPage';
import TitleBar from '@/components/TitleBar';
import { getStatus } from '@/api/streams';
import { Radio, Video } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';

export default function App() {
    const [streams, setStreams] = useState([]);
    const [activeTab, setActiveTab] = useState('streaming');
    const [loading, setLoading] = useState(true);
    const [serverError, setServerError] = useState(null);
    const [previewEnabled, setPreviewEnabled] = useState(() => localStorage.getItem('hipocast.previewEnabled') !== 'false');
    const latestRequest = useRef(0);

    const loadStatus = useCallback(async () => {
        const request = ++latestRequest.current;
        const data = await getStatus();
        if (request === latestRequest.current) {
            setStreams(data.streams || []);
            setServerError(data.serverError);
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        let disposed = false;
        let unsubscribe;
        let fallback;
        listen('hipocast:stream-status', loadStatus).then((unlisten) => {
            if (disposed) {
                unlisten();
                return;
            }
            unsubscribe = unlisten;
            loadStatus();
        }).catch(() => {
            if (!disposed) {
                loadStatus();
                fallback = setInterval(loadStatus, 1000);
            }
        });
        return () => {
            disposed = true;
            unsubscribe?.();
            clearInterval(fallback);
            latestRequest.current++;
        };
    }, [loadStatus]);

    const changePreview = (event) => {
        const enabled = event.target.checked;
        setPreviewEnabled(enabled);
        localStorage.setItem('hipocast.previewEnabled', String(enabled));
    };

    // Splash Screen durante carregamento
    if (loading) {
        return (
            <div className="min-h-screen bg-slate-950 flex flex-col items-center justify-center gap-6">
                <motion.div
                    animate={{ scale: [1, 1.1, 1], opacity: [0.5, 1, 0.5] }}
                    transition={{ duration: 2, repeat: Infinity }}
                    className="w-16 h-16 rounded-2xl bg-gradient-to-br from-violet-500 to-purple-600 flex items-center justify-center shadow-2xl shadow-violet-500/20"
                >
                    <Video className="w-8 h-8 text-white" />
                </motion.div>
                <div className="flex flex-col items-center gap-2">
                    <p className="text-slate-400 font-bold tracking-[0.2em] text-xs uppercase">Carregando</p>
                    <div className="flex gap-1">
                        <motion.div animate={{ opacity: [0, 1, 0] }} transition={{ repeat: Infinity, duration: 1, delay: 0 }} className="w-1 h-1 rounded-full bg-violet-500" />
                        <motion.div animate={{ opacity: [0, 1, 0] }} transition={{ repeat: Infinity, duration: 1, delay: 0.2 }} className="w-1 h-1 rounded-full bg-violet-500" />
                        <motion.div animate={{ opacity: [0, 1, 0] }} transition={{ repeat: Infinity, duration: 1, delay: 0.4 }} className="w-1 h-1 rounded-full bg-violet-500" />
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="flex flex-col h-screen bg-slate-950 text-slate-100 font-sans selection:bg-violet-500/30 overflow-hidden">
            {/* Custom Title Bar */}
            <TitleBar />

            {/* Main App Content */}
            <div className="flex flex-1 min-h-0">
                {/* Navigation */}
                <Sidebar activeTab={activeTab} onTabChange={setActiveTab} />

                {/* Main Content Area */}
                <main className="flex-1 flex flex-col min-w-0 overflow-hidden">
                    {/* Content View */}
                    <div className="p-8 overflow-y-auto flex-1 custom-scrollbar">
                        <AnimatePresence mode="wait">
                            {activeTab === 'streaming' ? (
                                <motion.div
                                    key="streaming"
                                    initial={{ opacity: 0, y: 10 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    exit={{ opacity: 0, y: -10 }}
                                    className="space-y-8 pb-10"
                                >
                                    <div className="flex flex-col gap-1">
                                        <h2 className="text-2xl font-bold text-white tracking-tight">Canais de Transmissão</h2>
                                        <p className="text-slate-400 text-sm italic">Engine de HLS Ultra-Otimizada via FFmpeg Pipeline</p>
                                    </div>

                                    <ControlPanel onStreamStarted={loadStatus} serverError={serverError} />

                                    <section className="space-y-4">
                                        <div className="flex items-center justify-between">
                                            <div className="flex items-center gap-2">
                                                <Radio className="w-5 h-5 text-violet-400" />
                                                <h2 className="text-lg font-bold text-white">Transmissões Ativas</h2>
                                            </div>
                                            <label className="flex cursor-pointer items-center gap-2 text-xs text-slate-300">
                                                <input type="checkbox" checked={previewEnabled} onChange={changePreview} className="accent-violet-500" />
                                                Prévia ao vivo
                                            </label>
                                        </div>
                                        <StreamGrid streams={streams} onStreamStopped={loadStatus} previewEnabled={previewEnabled} />
                                    </section>
                                </motion.div>
                            ) : activeTab === 'settings' ? (
                                <SettingsPage key="settings" />
                            ) : (
                                <motion.div
                                    key="other"
                                    initial={{ opacity: 0, y: 10 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    exit={{ opacity: 0, y: -10 }}
                                    className="flex flex-col items-center justify-center py-20 text-center space-y-4"
                                >
                                    <div className="w-20 h-20 rounded-full bg-slate-900 border border-slate-800 flex items-center justify-center">
                                        <Video className="w-8 h-8 text-slate-700" />
                                    </div>
                                    <div className="space-y-1">
                                        <h3 className="text-xl font-bold text-white">Em Desenvolvimento</h3>
                                        <p className="text-slate-400 max-w-sm">Esta funcionalidade está sendo preparada para a próxima versão do Live-Go Desktop.</p>
                                    </div>
                                </motion.div>
                            )}
                        </AnimatePresence>
                    </div>
                </main>
            </div>
        </div>
    );
}
