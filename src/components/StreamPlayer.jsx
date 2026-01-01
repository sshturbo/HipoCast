import { useEffect, useRef, useState } from 'react';
import Hls from 'hls.js';
import { Tv, Loader2, AlertCircle } from 'lucide-react';

export default function StreamPlayer({ streamId, hlsPath, status, onError }) {
    const videoRef = useRef(null);
    const hlsRef = useRef(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    useEffect(() => {
        const video = videoRef.current;
        if (!video || !hlsPath || status !== 'running') return;

        const baseUrl = hlsPath.startsWith('http') ? hlsPath : `http://localhost:8080${hlsPath}`;
        const streamUrl = `${baseUrl}?t=${Date.now()}`;
        console.log('StreamPlayer: Carregando', streamUrl);

        setLoading(true);
        setError(null);

        if (hlsRef.current) {
            hlsRef.current.destroy();
            hlsRef.current = null;
        }

        // Initialize HLS immediately
        if (Hls.isSupported()) {
            const hls = new Hls({
                enableWorker: true,
                lowLatencyMode: true,
                liveSyncDuration: 4, // More conservative to avoid freezes
                liveMaxLatencyDuration: 10, // Max 10s delay allowed
                liveDurationInfinity: true,
                maxBufferLength: 8,
                manifestLoadingTimeOut: 5000,
                manifestLoadingMaxRetry: 20, // More retries for initial 404s
                manifestLoadingRetryDelay: 1000,
                levelLoadingTimeOut: 5000,
                levelLoadingMaxRetry: 20,
                fragLoadingTimeOut: 10000,
                fragLoadingMaxRetry: 10,
            });

            hls.loadSource(streamUrl);
            hls.attachMedia(video);

            hls.on(Hls.Events.MANIFEST_PARSED, () => {
                console.log('StreamPlayer: Manifest parsed');
                setLoading(false);
                video.play().catch((e) => {
                    console.warn('StreamPlayer: Autoplay bloqueado', e);
                });
            });

            hls.on(Hls.Events.ERROR, (event, data) => {
                if (data.fatal) {
                    console.warn('StreamPlayer Fatal Error:', data.type, data.details);
                    switch (data.type) {
                        case Hls.ErrorTypes.NETWORK_ERROR:
                            // If it's a 404, we just keep waiting/retrying
                            console.log('StreamPlayer: Network error, retrying...');
                            hls.startLoad();
                            break;
                        case Hls.ErrorTypes.MEDIA_ERROR:
                            console.log('StreamPlayer: Media error, recovering...');
                            hls.recoverMediaError();
                            break;
                        default:
                            console.error('StreamPlayer: Unrecoverable error', data);
                            hls.destroy();
                            setError('Erro fatal na transmissão');
                            setLoading(false);
                            break;
                    }
                }
            });

            hlsRef.current = hls;
        } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
            video.src = streamUrl;
            video.addEventListener('loadedmetadata', () => {
                setLoading(false);
                video.play().catch(() => { });
            });
        } else {
            setError('Navegador não suportado');
            setLoading(false);
        }

        return () => {
            if (hlsRef.current) {
                hlsRef.current.destroy();
                hlsRef.current = null;
            }
        };
    }, [hlsPath, status, onError]);

    if (status === 'restarting') {
        return (
            <div className="w-full h-full flex flex-col items-center justify-center gap-3 text-yellow-500 bg-slate-900">
                <Loader2 className="w-12 h-12 animate-spin" />
                <span className="text-sm">Reiniciando...</span>
            </div>
        );
    }

    if (status !== 'running' && status !== 'restarting') {
        return (
            <div className="w-full h-full flex flex-col items-center justify-center gap-3 text-slate-500 bg-slate-900">
                <Tv className="w-12 h-12 opacity-50" />
                <span className="text-sm">Stream pausado</span>
            </div>
        );
    }

    return (
        <div className="relative w-full h-full bg-black">
            <video
                ref={videoRef}
                className="w-full h-full object-contain"
                controls
                autoPlay
                muted
                playsInline
            />

            {loading && (
                <div className="absolute inset-0 flex flex-col items-center justify-center bg-black/80">
                    <Loader2 className="w-8 h-8 text-violet-500 animate-spin" />
                    <span className="text-xs text-slate-400 mt-2">Aguardando stream...</span>
                </div>
            )}

            {error && (
                <div className="absolute inset-0 flex flex-col items-center justify-center bg-black/80">
                    <AlertCircle className="w-8 h-8 text-red-500" />
                    <span className="text-xs text-red-400 mt-2">{error}</span>
                </div>
            )}

            {!loading && !error && (
                <div className="absolute top-2 left-2 flex items-center gap-1.5 px-2 py-1 bg-black/50 backdrop-blur-sm rounded-md">
                    <div className="w-2 h-2 rounded-full bg-red-500 animate-pulse" />
                    <span className="text-xs text-white font-medium">AO VIVO</span>
                </div>
            )}
        </div>
    );
}
