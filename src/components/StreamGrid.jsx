import { useState } from 'react';
import { openUrl } from "@tauri-apps/plugin-opener";
import StreamPlayer from './StreamPlayer';
import { StreamAudioModal } from './StreamAudioModal';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardFooter } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { stopStream, removeStream, startStream, startBrowserStream } from '@/api/streams';
import { Square, ExternalLink, Copy, Check, Settings2, Pause, Play, Trash2, Volume2 } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { cn } from '@/lib/utils';
import { invoke } from '@tauri-apps/api/core';
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from '@/components/ui/alert-dialog';

export default function StreamGrid({ streams, onStreamStopped }) {
    const [copiedId, setCopiedId] = useState(null);
    const [streamToDelete, setStreamToDelete] = useState(null);
    const [audioModalOpen, setAudioModalOpen] = useState(false);
    const [selectedStream, setSelectedStream] = useState(null);
    const [errorMessage, setErrorMessage] = useState(null);

    const handleStop = async (id) => {
        try {
            const data = await stopStream(id);
            if (data && onStreamStopped) {
                // We pass null or specific logic if needed, but the best is to trigger a full refresh
                // or use the returned data if we update the component to handle it.
                // For now, let's just trigger the callback which we'll map to loadStatus
                onStreamStopped();
            }
        } catch (err) {
            console.error('Erro ao pausar captura:', err);
        }
    };

    const handleResume = async (stream) => {
        if (!stream?.id) {
            console.error('Erro ao retomar captura: ID do stream não definido');
            return;
        }
        try {
            // Buscar informações completas da stream do banco
            const streamInfo = await invoke('get_stream_info', { streamId: stream.id });
            
            // Retomar baseado no tipo
            if (streamInfo.sourceType === 'browser' && streamInfo.sourceUrl) {
                // Browser stream: reinicia com URL e mantém o mesmo ID
                await startBrowserStream(streamInfo.sourceUrl, stream.title || 'Web Stream', stream.id);
            } else {
                // Window capture: reinicia com o mesmo ID
                // Nota: Se a janela foi fechada, isso pode falhar
                await startStream(stream.id, stream.title || 'Stream');
            }
            onStreamStopped?.(); // Refresh after resume
        } catch (err) {
            console.error('Erro ao retomar captura:', err);
            setErrorMessage('Erro ao retomar stream. A janela pode ter sido fechada ou a URL pode estar inválida.');
        }
    };

    // Abre o modal de confirmação
    const handleRemoveRequest = (id) => {
        setStreamToDelete(id);
    };

    // Confirma e executa a exclusão
    const confirmRemove = async () => {
        if (!streamToDelete) return;
        try {
            await removeStream(streamToDelete);
            onStreamStopped?.();
        } catch (err) {
            console.error('Erro ao encerrar captura:', err);
        } finally {
            setStreamToDelete(null);
        }
    };

    const handleCopyLink = (stream) => {
        const url = stream.hlsPath.startsWith('http') ? stream.hlsPath : `http://localhost:8080${stream.hlsPath}`;
        navigator.clipboard.writeText(url);
        setCopiedId(stream.id);
        setTimeout(() => setCopiedId(null), 2000);
    };

    const handleOpenExternal = (stream) => {
        const url = stream.hlsPath.startsWith('http') ? stream.hlsPath : `http://localhost:8080${stream.hlsPath}`;
        openUrl(url);
    };

    const handleOpenAudioSettings = (stream) => {
        setSelectedStream(stream);
        setAudioModalOpen(true);
    };

    const handleCloseAudioModal = () => {
        setAudioModalOpen(false);
        setSelectedStream(null);
        onStreamStopped?.(); // Refresh streams after config change
    };

    if (streams.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-20 bg-slate-900/20 border-2 border-dashed border-slate-800 rounded-3xl">
                <div className="w-16 h-16 rounded-full bg-slate-900 flex items-center justify-center mb-4">
                    <Square className="w-6 h-6 text-slate-700" />
                </div>
                <p className="text-slate-400 font-medium tracking-tight">Canais Offline</p>
                <p className="text-slate-600 text-xs mt-1">Nenhum stream ativo no momento.</p>
            </div>
        );
    }

    return (
        <>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                <AnimatePresence>
                    {streams.map((stream) => (
                        <motion.div
                            key={stream.id}
                            layout
                            initial={{ opacity: 0, scale: 0.95 }}
                            animate={{ opacity: 1, scale: 1 }}
                            exit={{ opacity: 0, scale: 0.95 }}
                            transition={{ duration: 0.2 }}
                        >
                            <Card className="overflow-hidden bg-slate-900/40 backdrop-blur-md border-slate-800/50 group hover:border-violet-500/50 transition-all duration-500 shadow-2xl shadow-black/40 rounded-2xl">
                                <div className="relative aspect-video">
                                    <StreamPlayer
                                        streamId={stream.id}
                                        hlsPath={stream.hlsPath}
                                        status={stream.status}
                                    />

                                    {/* Overlay - Badge de Status no Topo Esquerdo */}
                                    <div className="absolute top-3 left-3">
                                        <Badge className={
                                            cn(
                                                "px-3 py-1 rounded-full text-xs font-bold tracking-wider border transition-all duration-500 shadow-lg",
                                                stream.status === 'running'
                                                    ? 'bg-green-500/90 text-white border-green-400/50 shadow-green-500/50'
                                                    : stream.status === 'restarting'
                                                        ? 'bg-yellow-500/90 text-white border-yellow-400/50 shadow-yellow-500/50'
                                                        : 'bg-red-500/90 text-white border-red-400/50 shadow-red-500/50'
                                            )
                                        }>
                                            <div className={cn("w-1.5 h-1.5 rounded-full mr-2", stream.status === 'running' ? "bg-white animate-pulse" : "bg-white")} />
                                            {stream.status === 'running' ? 'AO VIVO' : stream.status === 'restarting' ? 'INICIANDO' : 'PAUSADO'}
                                        </Badge>
                                    </div>

                                    {/* Overlay - Controles no Topo Direito */}
                                    <div className="absolute top-3 right-3 flex gap-2">
                                        <Button
                                            size="icon"
                                            variant="secondary"
                                            className="w-9 h-9 rounded-full bg-slate-900/90 backdrop-blur-md border-slate-700 hover:bg-violet-600 hover:border-violet-500 transition-all shadow-lg opacity-0 group-hover:opacity-100"
                                            onClick={() => handleOpenExternal(stream)}
                                            title="Abrir em nova aba"
                                        >
                                            <ExternalLink className="w-4 h-4" />
                                        </Button>
                                        <Button
                                            size="icon"
                                            variant="secondary"
                                            className="w-9 h-9 rounded-full bg-slate-900/90 backdrop-blur-md border-slate-700 hover:bg-violet-600 hover:border-violet-500 transition-all shadow-lg opacity-0 group-hover:opacity-100"
                                            onClick={() => handleCopyLink(stream)}
                                            title="Copiar link HLS"
                                        >
                                            {copiedId === stream.id ? <Check className="w-4 h-4 text-green-400" /> : <Copy className="w-4 h-4" />}
                                        </Button>
                                        <Button
                                            size="icon"
                                            variant="destructive"
                                            className="w-9 h-9 rounded-full bg-red-500/90 backdrop-blur-md border-red-400/50 hover:bg-red-600 transition-all shadow-lg shadow-red-500/30"
                                            onClick={() => handleRemoveRequest(stream.id)}
                                            title="Encerrar transmissão"
                                        >
                                            <Trash2 className="w-4 h-4" />
                                        </Button>
                                    </div>
                                </div>

                                <CardContent className="p-5 space-y-3">
                                    <div className="flex items-start justify-between gap-3">
                                        <div className="min-w-0 flex-1">
                                            <h3 className="font-bold text-base text-white truncate mb-1">{stream.title || stream.id}</h3>
                                            <p className="text-xs text-slate-500 font-medium">ID: {stream.id}</p>
                                        </div>
                                    </div>

                                    {/* Link for copying */}
                                    <div
                                        className="flex items-center gap-3 p-3 bg-slate-950/50 rounded-xl border border-slate-800/50 cursor-pointer hover:border-violet-500/50 hover:bg-slate-950/80 transition-all duration-300 group/link"
                                        onClick={() => handleCopyLink(stream)}
                                    >
                                        <div className="p-1.5 rounded-md bg-slate-900 border border-slate-800 text-slate-500 group-hover/link:text-violet-400 group-hover/link:border-violet-500/30 transition-colors">
                                            {copiedId === stream.id ? <Check className="w-3.5 h-3.5 text-green-400" /> : <Copy className="w-3.5 h-3.5" />}
                                        </div>
                                        <code className="text-xs text-slate-500 truncate flex-1 font-mono leading-none group-hover/link:text-slate-300 transition-colors">
                                            {stream.hlsPath.startsWith('http') ? stream.hlsPath : `http://localhost:8080${stream.hlsPath}`}
                                        </code>
                                    </div>
                                </CardContent>

                                <CardFooter className="px-5 py-3 bg-slate-950/30 border-t border-slate-800/50 flex gap-2">
                                    {/* Botão Ajustes de Áudio */}
                                    <Button
                                        variant="secondary"
                                        size="sm"
                                        className="bg-violet-500/10 hover:bg-violet-500/20 text-violet-400 border border-violet-500/20 hover:border-violet-500/40 transition-all font-bold text-xs h-9 px-4"
                                        onClick={() => handleOpenAudioSettings(stream)}
                                        title="Configurar áudio desta stream"
                                    >
                                        <Volume2 className="w-3.5 h-3.5 mr-2" />
                                        Ajustes
                                    </Button>

                                    {/* Botão Pausar/Retomar */}
                                    {stream.status === 'running' ? (
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            className="flex-1 bg-yellow-500/10 hover:bg-yellow-500/20 text-yellow-400 border border-yellow-500/20 hover:border-yellow-500/40 transition-all font-bold text-xs h-9"
                                            onClick={() => handleStop(stream.id)}
                                        >
                                            <Pause className="w-3.5 h-3.5 mr-2 fill-current" />
                                            Pausar
                                        </Button>
                                    ) : (
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            className="flex-1 bg-green-500/10 hover:bg-green-500/20 text-green-400 border border-green-500/20 hover:border-green-500/40 transition-all font-bold text-xs h-9"
                                            onClick={() => handleResume(stream)}
                                        >
                                            <Play className="w-3.5 h-3.5 mr-2 fill-current" />
                                            Retomar
                                        </Button>
                                    )}
                                </CardFooter>
                            </Card>
                        </motion.div>
                    ))}
                </AnimatePresence>
            </div>

            <AlertDialog open={!!streamToDelete} onOpenChange={(open) => !open && setStreamToDelete(null)}>
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>Encerrar Transmissão?</AlertDialogTitle>
                        <AlertDialogDescription>
                            Isso irá parar a live imediatamente e excluir todos os arquivos gerados.
                            Esta ação não pode ser desfeita.
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel onClick={() => setStreamToDelete(null)}>Cancelar</AlertDialogCancel>
                        <AlertDialogAction onClick={confirmRemove}>Sim, Encerrar</AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>

            {/* Modal de Configuração de Áudio */}
            {selectedStream && (
                <StreamAudioModal
                    streamId={selectedStream.id}
                    streamTitle={selectedStream.title || selectedStream.id}
                    isOpen={audioModalOpen}
                    onClose={handleCloseAudioModal}
                />
            )}
        </>
    );
}
