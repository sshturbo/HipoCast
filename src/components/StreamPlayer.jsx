import { useEffect, useRef, useState } from 'react';
import { AlertCircle, Loader2, Tv } from 'lucide-react';

export default function StreamPlayer({ hlsPath, status, message, enabled }) {
    const videoRef = useRef(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);
    const [visible, setVisible] = useState(() => !document.hidden);

    useEffect(() => {
        const updateVisibility = () => setVisible(!document.hidden);
        document.addEventListener('visibilitychange', updateVisibility);
        return () => document.removeEventListener('visibilitychange', updateVisibility);
    }, []);

    useEffect(() => {
        const video = videoRef.current;
        if (!video || !hlsPath || status !== 'running' || !enabled || !visible) return;

        let player;
        let disposed = false;
        let failed = false;
        let recoveryAttempted = false;
        setLoading(true);
        setError(null);

        const ready = () => {
            clearTimeout(timeout);
            setLoading(false);
        };
        const fail = (message) => {
            if (disposed || failed) return;
            failed = true;
            player?.destroy();
            player = undefined;
            ready();
            setError(message);
        };
        const mediaError = () => fail('Não foi possível reproduzir a transmissão.');
        const timeout = setTimeout(() => fail('O player não recebeu vídeo a tempo.'), 20000);
        video.addEventListener('loadeddata', ready);
        video.addEventListener('error', mediaError);

        import('hls.js').then(({ default: Hls }) => {
            if (disposed || failed) return;

            if (Hls.isSupported()) {
                player = new Hls({
                    enableWorker: true,
                    lowLatencyMode: true,
                    maxBufferLength: 8,
                });
                player.on(Hls.Events.MANIFEST_PARSED, () => video.play().catch(() => {}));
                player.on(Hls.Events.ERROR, (_, data) => {
                    if (!data.fatal || !player) return;
                    if (!recoveryAttempted && data.type === Hls.ErrorTypes.MEDIA_ERROR) {
                        recoveryAttempted = true;
                        player.recoverMediaError();
                        return;
                    }
                    fail('O player perdeu acesso à transmissão HLS.');
                });
                player.loadSource(hlsPath);
                player.attachMedia(video);
            } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
                video.src = hlsPath;
            } else {
                fail('Este navegador não oferece suporte a HLS.');
            }
        }).catch(() => fail('Não foi possível carregar o player HLS.'));

        return () => {
            disposed = true;
            clearTimeout(timeout);
            video.removeEventListener('loadeddata', ready);
            video.removeEventListener('error', mediaError);
            video.pause();
            player?.destroy();
            video.removeAttribute('src');
            video.load();
        };
    }, [hlsPath, status, enabled, visible]);

    if (status === 'starting') {
        return (
            <div role="status" className="flex h-full flex-col items-center justify-center gap-3 bg-slate-900 text-yellow-400">
                <Loader2 className="h-9 w-9 animate-spin" />
                <span className="text-sm">Iniciando transmissão...</span>
            </div>
        );
    }

    if (status === 'failed') {
        return (
            <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 overflow-y-auto bg-slate-900 p-3 text-center text-red-300">
                <AlertCircle className="h-8 w-8 shrink-0" />
                <span className="break-words text-xs">{message || 'Falha ao capturar a transmissão.'}</span>
            </div>
        );
    }

    if (status !== 'running') {
        return (
            <div className="flex h-full flex-col items-center justify-center gap-3 bg-slate-900 text-slate-500">
                <Tv className="h-12 w-12 opacity-50" />
                <span className="text-sm">Transmissão pausada</span>
            </div>
        );
    }

    if (!enabled || !visible) {
        return (
            <div className="flex h-full flex-col items-center justify-center gap-3 bg-slate-900 text-slate-400">
                <Tv className="h-9 w-9 opacity-50" />
                <span className="text-sm">Prévia suspensa</span>
            </div>
        );
    }

    return (
        <div className="relative h-full w-full bg-black">
            <video ref={videoRef} className="h-full w-full object-contain" controls autoPlay muted playsInline />
            {loading && (
                <div role="status" className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/80">
                    <Loader2 className="h-8 w-8 animate-spin text-violet-500" />
                    <span className="text-xs text-slate-400">Carregando vídeo...</span>
                </div>
            )}
            {error && (
                <div role="alert" className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/80 p-3 text-center text-red-300">
                    <AlertCircle className="h-8 w-8" />
                    <span className="text-xs">{error}</span>
                </div>
            )}
        </div>
    );
}
