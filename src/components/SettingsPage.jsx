import { useState, useEffect } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Select } from '@/components/ui/select';
import { fetchSettings, updateSettings, getStatus, stopStream, startStream } from '@/api/streams';
import { Save, RefreshCcw, Video, Server, Clock, Loader2, Volume2, Mic, Settings2, Cpu } from 'lucide-react';
import { motion } from 'framer-motion';

export default function SettingsPage() {
    const [settings, setSettings] = useState(null);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [message, setMessage] = useState(null);

    useEffect(() => {
        loadSettings();
    }, []);

    const loadSettings = async () => {
        setLoading(true);
        const s = await fetchSettings();
        setSettings(s);
        setLoading(false);
    };

    const handleSave = async () => {
        setSaving(true);
        setMessage(null);
        try {
            // Verificar se há transmissão ativa
            const status = await getStatus();
            const activeStream = status.streams?.find(s => s.status === 'running');
            
            // Salvar configurações
            await updateSettings(settings);
            
            // Se houver transmissão ativa, reiniciar para aplicar mudanças
            if (activeStream) {
                setMessage({ type: 'info', text: 'Reiniciando transmissão para aplicar mudanças...' });
                
                // Parar transmissão
                await stopStream(activeStream.id);
                
                // Aguardar 1 segundo para garantir que parou
                await new Promise(resolve => setTimeout(resolve, 1000));
                
                // Reiniciar transmissão
                await startStream(activeStream.id, activeStream.title);
                
                setMessage({ type: 'success', text: 'Configurações aplicadas e transmissão reiniciada!' });
            } else {
                setMessage({ type: 'success', text: 'Configurações salvas com sucesso!' });
            }
        } catch (err) {
            console.error('Erro ao salvar:', err);
            setMessage({ type: 'error', text: 'Erro ao salvar configurações.' });
        } finally {
            setSaving(false);
        }
    };

    if (loading) {
        return (
            <div className="flex flex-col items-center justify-center py-20 gap-4">
                <Loader2 className="w-10 h-10 text-violet-500 animate-spin" />
                <p className="text-slate-400 animate-pulse">Carregando configurações...</p>
            </div>
        );
    }

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="space-y-6 max-w-4xl mx-auto"
        >
            <div className="bg-slate-900/40 backdrop-blur-xl p-8 rounded-3xl border border-slate-800/50 shadow-2xl">
                <div className="flex items-center gap-5 mb-6">
                    <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-violet-600 to-indigo-700 flex items-center justify-center shadow-lg shadow-violet-600/20">
                        <Settings2 className="w-6 h-6 text-white" />
                    </div>
                    <div>
                        <h2 className="text-2xl font-black text-white tracking-tight uppercase">Configurações do Sistema</h2>
                        <p className="text-slate-500 text-sm font-medium mt-1">Pipeline de Captura e Codificação Nativa</p>
                    </div>
                </div>
                <Button
                    onClick={handleSave}
                    disabled={saving}
                    className="w-full h-12 px-8 bg-violet-600 hover:bg-violet-500 text-white font-bold uppercase text-sm shadow-xl shadow-violet-600/20 rounded-xl transition-all border-none"
                >
                    {saving ? <Loader2 className="w-4 h-4 mr-3 animate-spin" /> : <Save className="w-4 h-4 mr-3" />}
                    Salvar Alterações
                </Button>
            </div>

            {message && (
                <Badge
                    className={message.type === 'success' ? 'w-full py-3 justify-center bg-green-500/10 text-green-400 border-green-500/20' : 'w-full py-3 justify-center bg-red-500/10 text-red-500 border-red-500/20'}
                >
                    {message.text}
                </Badge>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                {/* Vídeo */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-violet-500/10 flex items-center justify-center border border-violet-500/20">
                                <Video className="w-3 h-3 text-violet-400" />
                            </div>
                            Matriz de Vídeo
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        <div className="space-y-2">
                            <label className="text-xs font-medium text-slate-500">Framerate (FPS)</label>
                            <Select
                                value={settings.framerate?.toString()}
                                onChange={(e) => setSettings(prev => ({ ...prev, framerate: parseInt(e.target.value) }))}
                                className="bg-slate-950 border-slate-800"
                            >
                                <option value="15">15 FPS (Leve)</option>
                                <option value="30">30 FPS (Padrão)</option>
                                <option value="60">60 FPS (Alta Performance)</option>
                            </Select>
                        </div>
                        <div className="space-y-2">
                            <label className="text-xs font-medium text-slate-500">Bitrate de Vídeo (kbps)</label>
                            <Select
                                value={settings.bitrate?.toString()}
                                onChange={(e) => setSettings(prev => ({ ...prev, bitrate: parseInt(e.target.value) }))}
                                className="bg-slate-950 border-slate-800"
                            >
                                <option value="2000000">SD - 2 Mbps (720p)</option>
                                <option value="4500000">HD - 4.5 Mbps (1080p Standard)</option>
                                <option value="6000000">ULTRA - 6 Mbps (1080p High)</option>
                                <option value="15000000">EXTREME - 15 Mbps (4K Cinema)</option>
                            </Select>
                        </div>
                        <div className="grid grid-cols-2 gap-4">
                            <div className="space-y-2">
                                <label className="text-xs font-medium text-slate-500 text-[10px] uppercase tracking-wider">Largura (Width)</label>
                                <Input
                                    type="number"
                                    value={settings.width}
                                    onChange={(e) => setSettings(prev => ({ ...prev, width: parseInt(e.target.value) || 1280 }))}
                                    className="bg-slate-950 border-slate-800"
                                    placeholder="1280"
                                />
                            </div>
                            <div className="space-y-2">
                                <label className="text-xs font-medium text-slate-500 text-[10px] uppercase tracking-wider">Altura (Height)</label>
                                <Input
                                    type="number"
                                    value={settings.height}
                                    onChange={(e) => setSettings(prev => ({ ...prev, height: parseInt(e.target.value) || 720 }))}
                                    className="bg-slate-950 border-slate-800"
                                    placeholder="720"
                                />
                            </div>
                        </div>
                        <p className="text-[10px] text-slate-500 mt-1 italic">
                            O engine de escala irá adaptar automaticamente seu navegador para esta resolução (Ex: 1920x1080 para Full HD).
                        </p>
                    </CardContent>
                </Card>

                {/* Encoding Engine */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-emerald-500/10 flex items-center justify-center border border-emerald-500/20">
                                <Cpu className="w-3 h-3 text-emerald-400" />
                            </div>
                            Motor de Codificação
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 block">Aceleração de Hardware (GPU)</span>
                                <span className="text-xs text-slate-500 block">Use o codificador de hardware NVIDIA/AMD/Intel se disponível.</span>
                            </div>
                            <div className="flex items-center gap-2">
                                <div
                                    onClick={() => setSettings(prev => ({ ...prev, enableHwAccel: !prev.enableHwAccel }))}
                                    className={`w-10 h-5 rounded-full cursor-pointer transition-colors relative ${settings.enableHwAccel ? 'bg-emerald-500' : 'bg-slate-700'}`}
                                >
                                    <div className={`absolute top-1 w-3 h-3 bg-white rounded-full transition-all shadow-md ${settings.enableHwAccel ? 'left-6' : 'left-1'}`} />
                                </div>
                            </div>
                        </div>

                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 block">Preset do Codificador (CPU)</span>
                                <span className="text-xs text-slate-500 block">Equilíbrio entre velocidade e qualidade. Mais rápido = Menos uso de CPU.</span>
                            </div>
                            <Select
                                value={settings.ffmpegPreset || "ultrafast"}
                                onChange={(e) => setSettings(prev => ({ ...prev, ffmpegPreset: e.target.value }))}
                                className="bg-slate-900 border-slate-700 w-full h-10 text-sm"
                            >
                                <option value="ultrafast">Ultrafast (Melhor)</option>
                                <option value="superfast">Superfast</option>
                                <option value="veryfast">Veryfast</option>
                                <option value="faster">Faster</option>
                                <option value="fast">Fast</option>
                                <option value="medium">Medium (Pesado)</option>
                            </Select>
                        </div>
                    </CardContent>
                </Card>

                {/* Áudio do Sistema */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-blue-500/10 flex items-center justify-center border border-blue-500/20">
                                <Volume2 className="w-3 h-3 text-blue-400" />
                            </div>
                            Áudio do Sistema
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        {/* System Audio Toggle */}
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 flex items-center gap-2">
                                    <Volume2 className="w-4 h-4" /> Captura de Áudio
                                </span>
                                <span className="text-xs text-slate-500 block">Ativa captura de som de aplicativos e navegador (WASAPI Loopback).</span>
                            </div>
                            <div className="flex items-center gap-2">
                                <div
                                    onClick={() => setSettings(prev => ({ ...prev, enableAudio: !prev.enableAudio }))}
                                    className={`w-10 h-5 rounded-full cursor-pointer transition-colors relative ${settings.enableAudio ? 'bg-blue-500' : 'bg-slate-700'}`}
                                >
                                    <div className={`absolute top-1 w-3 h-3 bg-white rounded-full transition-all shadow-md ${settings.enableAudio ? 'left-6' : 'left-1'}`} />
                                </div>
                            </div>
                        </div>

                        {/* Audio Bitrate */}
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 block">Bitrate de Áudio</span>
                                <span className="text-xs text-slate-500 block">Qualidade do áudio codificado (AAC).</span>
                            </div>
                            <Select
                                value={settings.audioBitrate?.toString() || "128"}
                                onChange={(e) => setSettings(prev => ({ ...prev, audioBitrate: parseInt(e.target.value) }))}
                                className="bg-slate-900 border-slate-700 w-full h-10 text-sm"
                            >
                                <option value="64">64 kbps (Baixa)</option>
                                <option value="128">128 kbps (Padrão)</option>
                                <option value="192">192 kbps (Alta)</option>
                                <option value="256">256 kbps (Premium)</option>
                            </Select>
                        </div>

                        {/* Audio Buffer Size */}
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 block">Buffer de Áudio (ms)</span>
                                <span className="text-xs text-slate-500 block">Latência do áudio. Menor = menos delay.</span>
                            </div>
                            <Input
                                type="number"
                                value={settings.audioBufferSize || 50}
                                onChange={(e) => setSettings(prev => ({ ...prev, audioBufferSize: parseInt(e.target.value) || 50 }))}
                                className="bg-slate-900 border-slate-700 w-full h-10 text-sm text-center"
                                min="10"
                                max="500"
                            />
                        </div>

                        {/* Audio Offset */}
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 block">Offset de Áudio (ms)</span>
                                <span className="text-xs text-slate-500 block">Ajuste de sincronização. + = atrasa áudio.</span>
                            </div>
                            <Input
                                type="number"
                                value={settings.audioOffset || 0}
                                onChange={(e) => setSettings(prev => ({ ...prev, audioOffset: parseInt(e.target.value) || 0 }))}
                                className="bg-slate-900 border-slate-700 w-full h-10 text-sm text-center"
                                min="-1000"
                                max="1000"
                            />
                        </div>
                    </CardContent>
                </Card>

                {/* Processamento de Áudio Global */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-purple-500/10 flex items-center justify-center border border-purple-500/20">
                                <Settings2 className="w-3 h-3 text-purple-400" />
                            </div>
                            Processamento de Áudio Global
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        <div className="space-y-2">
                            <label className="text-xs font-medium text-slate-500">Preset de Efeitos</label>
                            <Select
                                value={settings.globalAudioPreset || "none"}
                                onChange={(e) => setSettings(prev => ({ ...prev, globalAudioPreset: e.target.value }))}
                                className="bg-slate-950 border-slate-800"
                            >
                                <option value="none">Nenhum (Bypass)</option>
                                <option value="clean_light">Clean Light (CPU: ~2%)</option>
                                <option value="clean_balanced">Clean Balanced (CPU: ~4%)</option>
                                <option value="professional">Professional (CPU: ~8%)</option>
                                <option value="gaming">Gaming (CPU: ~10%)</option>
                                <option value="podcast">Podcast (CPU: ~12%)</option>
                                <option value="custom">Custom (Filtros Personalizados)</option>
                            </Select>
                            <p className="text-[10px] text-slate-500 mt-1 italic">
                                💡 Configuração padrão aplicada a todos os novos streams. Pode ser sobrescrita individualmente.
                            </p>
                        </div>

                        <div className="space-y-2">
                            <label className="text-xs font-medium text-slate-500">Volume Global (0.0 - 1.0)</label>
                            <div className="flex items-center gap-3">
                                <Input
                                    type="number"
                                    step="0.1"
                                    min="0"
                                    max="2"
                                    value={settings.globalAudioVolume ?? 0.8}
                                    onChange={(e) => setSettings(prev => ({ ...prev, globalAudioVolume: parseFloat(e.target.value) || 0.8 }))}
                                    className="bg-slate-950 border-slate-800 h-10 text-center"
                                />
                                <Badge className="bg-violet-500/10 text-violet-400 border-violet-500/20 px-3 py-1">
                                    {Math.round((settings.globalAudioVolume ?? 0.8) * 100)}%
                                </Badge>
                            </div>
                            <p className="text-[10px] text-slate-500 mt-1">
                                ⚠️ Valores acima de 1.0 podem causar distorção (clipping). Recomendado: 0.8
                            </p>
                        </div>

                        {settings.globalAudioPreset === 'custom' && (
                            <div className="space-y-2">
                                <label className="text-xs font-medium text-slate-500">Filtros FFmpeg Personalizados</label>
                                <textarea
                                    value={settings.globalAudioCustomFilters || ''}
                                    onChange={(e) => setSettings(prev => ({ ...prev, globalAudioCustomFilters: e.target.value }))}
                                    className="w-full h-24 px-3 py-2 bg-slate-950 border border-slate-800 rounded-lg text-xs text-slate-300 font-mono resize-none focus:outline-none focus:ring-2 focus:ring-violet-500/50"
                                    placeholder="highpass=f=80,lowpass=f=15000,volume=0.8"
                                />
                                <p className="text-[10px] text-slate-500 mt-1">
                                    📖 Use sintaxe FFmpeg audio filter. Ex: <code className="text-violet-400">highpass=f=100,compand,volume=0.9</code>
                                </p>
                            </div>
                        )}

                        <div className="p-3 rounded-lg bg-blue-500/5 border border-blue-500/20">
                            <p className="text-xs text-blue-300 leading-relaxed">
                                💡 <strong>Info:</strong> Estas configurações serão aplicadas automaticamente a todos os novos streams criados. Streams existentes manterão suas configurações individuais.
                            </p>
                        </div>
                    </CardContent>
                </Card>

                {/* Microfone */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-pink-500/10 flex items-center justify-center border border-pink-500/20">
                                <Mic className="w-3 h-3 text-pink-400" />
                            </div>
                            Microfone
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        {/* Microphone Toggle */}
                        <div className="p-4 rounded-xl bg-slate-950/20 border border-slate-800/50 space-y-3">
                            <div className="space-y-1">
                                <span className="text-sm font-medium text-slate-300 flex items-center gap-2">
                                    <Mic className="w-4 h-4" /> Captura de Microfone
                                </span>
                                <span className="text-xs text-slate-500 block">Captura áudio do microfone padrão e mixa com o áudio do sistema (WASAPI).</span>
                            </div>
                            <div className="flex items-center gap-2">
                                <div
                                    onClick={() => setSettings(prev => ({ ...prev, enableMicrophone: !prev.enableMicrophone }))}
                                    className={`w-10 h-5 rounded-full cursor-pointer transition-colors relative ${settings.enableMicrophone ? 'bg-pink-500' : 'bg-slate-700'}`}
                                >
                                    <div className={`absolute top-1 w-3 h-3 bg-white rounded-full transition-all shadow-md ${settings.enableMicrophone ? 'left-6' : 'left-1'}`} />
                                </div>
                            </div>
                        </div>

                        <div className="p-3 rounded-lg bg-slate-950/30 border border-slate-800/30">
                            <p className="text-xs text-slate-400 leading-relaxed">
                                💡 <strong>Dica:</strong> O microfone padrão do Windows será capturado automaticamente. Configure em Configurações do Windows → Som → Entrada.
                            </p>
                        </div>
                    </CardContent>
                </Card>

                {/* Segmentação HLS */}
                <Card className="bg-slate-900/40 backdrop-blur-md border-slate-800/50 shadow-2xl rounded-2xl overflow-hidden">
                    <CardHeader className="bg-slate-950/30 border-b border-slate-800/50 px-6 py-4">
                        <CardTitle className="text-sm font-bold uppercase text-slate-300 flex items-center gap-3">
                            <div className="w-6 h-6 rounded-lg bg-violet-500/10 flex items-center justify-center border border-violet-500/20">
                                <Server className="w-3 h-3 text-violet-400" />
                            </div>
                            Orquestração HLS
                        </CardTitle>
                    </CardHeader>
                    <CardContent className="pt-6 space-y-4">
                        <div className="space-y-2">
                            <label className="text-sm font-medium text-slate-300">Duração do Segmento (segundos)</label>
                            <Input
                                type="number"
                                value={settings.hlsTime}
                                onChange={(e) => setSettings(prev => ({ ...prev, hlsTime: parseFloat(e.target.value) }))}
                                className="bg-slate-950 border-slate-800 h-10"
                            />
                        </div>
                        <div className="space-y-2">
                            <label className="text-sm font-medium text-slate-300">Tamanho da Lista (Arquivos)</label>
                            <Input
                                type="number"
                                value={settings.hlsListSize}
                                onChange={(e) => setSettings(prev => ({ ...prev, hlsListSize: parseInt(e.target.value) || 0 }))}
                                className="bg-slate-950 border-slate-800 h-10"
                                placeholder="5 (Padrão) - 0 para Infinito"
                            />
                            <p className="text-xs text-slate-500 mt-1">
                                Define quantos arquivos manter. Use <b>0</b> para criar uma live infinita (estilo gravação). (Ex: 5 arquivos de 2s = 10s de janela). Arquivos antigos são apagados automaticamente.
                            </p>
                        </div>
                    </CardContent>
                </Card>


            </div>
        </motion.div >
    );
}
