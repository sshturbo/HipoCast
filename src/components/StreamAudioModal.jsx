import React, { useState, useEffect } from 'react';
import { X, Volume2, VolumeX, Mic, MicOff, Sliders, Info, ChevronDown, ChevronUp } from 'lucide-react';
import { Button } from './ui/button';
import { Select } from './ui/select';
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from './ui/alert-dialog';
import { 
    getStreamAudioConfig, 
    saveStreamAudioConfig, 
    listAudioSessions,
    getStatus,
    stopStream,
    startStream,
    startBrowserStream
} from '../api/streams';
import { invoke } from '@tauri-apps/api/core';

export function StreamAudioModal({ streamId, streamTitle, isOpen, onClose }) {
    const [config, setConfig] = useState({
        streamId: streamId,
        audioMode: 'system',
        targetPid: null,
        targetProcessName: null,
        enableMicrophone: false,
        microphoneDevice: '',
        audioEffects: {
            enabled: false,
            preset: 'cleanbalanced',
            customFilters: null,
            masterVolume: 0.8
        },
        microphoneEffects: {
            enabled: false,
            preset: 'balanced',
            customFilters: null,
            volume: 1.0
        }
    });

    const [audioSessions, setAudioSessions] = useState([]);
    const [loading, setLoading] = useState(false);
    const [saving, setSaving] = useState(false);
    const [suggestedPreset, setSuggestedPreset] = useState(null);
    const [showAdvanced, setShowAdvanced] = useState(false);
    const [errorMessage, setErrorMessage] = useState(null);
    const [microphonePresets, setMicrophonePresets] = useState([]);

    useEffect(() => {
        if (isOpen) {
            loadData();
            detectCPUAndSuggest();
            loadMicrophonePresets();
        }
    }, [isOpen, streamId]);

    const loadMicrophonePresets = async () => {
        try {
            const presets = await invoke('get_microphone_presets_info');
            setMicrophonePresets(presets);
        } catch (error) {
            console.error('Erro ao carregar presets de microfone:', error);
        }
    };

    const detectCPUAndSuggest = async () => {
        try {
            const [preset, reason, cpuImpact] = await invoke('get_suggested_audio_preset');
            setSuggestedPreset({ preset, reason, cpuImpact });
        } catch (error) {
            console.error('Erro ao detectar CPU:', error);
        }
    };

    const loadData = async () => {
        setLoading(true);
        try {
            // Carregar config existente (já retorna default se não existir)
            const existingConfig = await getStreamAudioConfig(streamId);
            
            // Garantir que todos os campos necessários existam
            const fullConfig = {
                ...existingConfig,
                streamId, // Garantir que o ID está correto
                audioEffects: {
                    enabled: false,
                    preset: 'cleanbalanced',
                    customFilters: null,
                    masterVolume: 0.8,
                    ...(existingConfig.audioEffects || {})
                },
                microphoneEffects: {
                    enabled: false,
                    preset: 'balanced',
                    customFilters: null,
                    volume: 1.0,
                    ...(existingConfig.microphoneEffects || {})
                }
            };
            setConfig(fullConfig);

            // Carregar sessões de áudio
            const sessions = await listAudioSessions();
            setAudioSessions(sessions);
        } catch (error) {
            console.error('Erro ao carregar dados:', error);
        } finally {
            setLoading(false);
        }
    };

    const handleSave = async () => {
        setSaving(true);
        try {
            // 1. Salvar configurações no banco
            await saveStreamAudioConfig(config);
            
            // 2. Verificar se esta transmissão está ativa no momento
            const status = await getStatus();
            const activeStream = status.streams?.find(s => s.id === streamId && s.status === 'running');
            
            // 3. Se estiver ativa, reiniciar para aplicar as mudanças de áudio
            if (activeStream) {
                console.log('Reiniciando stream ativa para aplicar novos efeitos de áudio...');
                
                // Buscar informações completas da stream do banco para saber como reiniciar
                const streamInfo = await invoke('get_stream_info', { streamId });
                
                // Parar a captura atual
                await stopStream(streamId);
                
                // Aguardar um pouco para o FFmpeg e os handles de áudio liberarem os recursos
                await new Promise(resolve => setTimeout(resolve, 1200));
                
                // Reiniciar de acordo com o tipo original (browser ou janela)
                // IMPORTANTE: Passar o streamId original para manter a mesma URL
                if (streamInfo.sourceType === 'browser' && streamInfo.sourceUrl) {
                    await startBrowserStream(streamInfo.sourceUrl, streamTitle, streamId);
                } else {
                    await startStream(streamId, streamTitle);
                }
            }
            
            onClose();
        } catch (error) {
            console.error('Erro detalhado ao salvar:', error);
            // Se o erro for um objeto, tenta pegar a mensagem, senão usa a padrão
            const msg = typeof error === 'string' ? error : (error?.message || 'Erro ao salvar configurações de áudio.');
            setErrorMessage(`${msg}. Por favor, tente novamente.`);
        } finally {
            setSaving(false);
        }
    };

    const handleAudioModeChange = (mode) => {
        setConfig(prev => ({ 
            ...prev, 
            audioMode: mode,
            targetPid: null,
            targetProcessName: null
        }));
    };

    const handleProcessSelect = (processId) => {
        const session = audioSessions.find(s => s.process_id === parseInt(processId));
        setConfig(prev => ({
            ...prev,
            targetPid: parseInt(processId),
            targetProcessName: session?.process_name || null
        }));
    };

    const getPresetInfo = (preset) => {
        const presets = {
            none: {
                name: '🔇 Nenhum',
                description: 'Sem processamento de áudio',
                cpuImpact: 'Nenhum'
            },
            cleanlight: {
                name: '✨ Limpo (Leve)',
                description: 'Filtro básico de ruído - Ideal para PCs fracos',
                cpuImpact: 'Baixo (~2%)'
            },
            cleanbalanced: {
                name: '⚖️ Limpo (Balanceado)',
                description: 'Filtro balanceado com gate - Recomendado para maioria',
                cpuImpact: 'Médio (~5%)'
            },
            professional: {
                name: '🎙️ Profissional',
                description: 'Processamento completo com compressor - Requer CPU potente',
                cpuImpact: 'Alto (~15%)'
            },
            gaming: {
                name: '🎮 Gaming',
                description: 'Otimizado para jogos com realce de graves',
                cpuImpact: 'Baixo-Médio (~4%)'
            },
            podcast: {
                name: '🎧 Podcast',
                description: 'Máxima qualidade vocal com deesser - CPU muito potente',
                cpuImpact: 'Muito Alto (~18%)'
            },
            custom: {
                name: '⚙️ Personalizado',
                description: 'Filtros personalizados pelo usuário',
                cpuImpact: 'Variável'
            }
        };
        return presets[preset] || presets.none;
    };

    const getMicPresetInfo = (preset) => {
        // Usar dados do backend se disponíveis
        if (microphonePresets.length > 0) {
            const presetData = microphonePresets.find(p => p.id === preset);
            if (presetData) {
                return {
                    name: presetData.name,
                    description: presetData.description,
                    cpuImpact: presetData.cpu_impact
                };
            }
        }
        
        // Fallback para dados estáticos
        const presets = {
            none: {
                name: '🔇 Nenhum',
                description: 'Sem processamento no microfone',
                cpuImpact: 'Nenhum'
            },
            light: {
                name: '✨ Leve',
                description: 'Filtro básico com noise gate - Ideal para ambientes silenciosos',
                cpuImpact: 'Baixo (~1%)'
            },
            balanced: {
                name: '⚖️ Balanceado',
                description: 'Gate + compressor leve - Recomendado para maioria',
                cpuImpact: 'Médio (~3%)'
            },
            podcast: {
                name: '🎙️ Podcast',
                description: 'Processamento profissional vocal com EQ - Qualidade broadcast',
                cpuImpact: 'Alto (~8%)'
            },
            professional: {
                name: '🎤 Profissional',
                description: 'Cadeia completa: Gate, Compressor, EQ e De-esser',
                cpuImpact: 'Muito Alto (~12%)'
            },
            custom: {
                name: '⚙️ Personalizado',
                description: 'Filtros personalizados pelo usuário',
                cpuImpact: 'Variável'
            }
        };
        return presets[preset] || presets.balanced;
    };

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
            <div className="bg-gray-800 rounded-lg w-full max-w-md border border-gray-700 flex flex-col max-h-[90vh]">
                {/* Header - Fixo */}
                <div className="flex justify-between items-center p-6 pb-4 border-b border-gray-700">
                    <div>
                        <h2 className="text-xl font-bold text-white">Configurações de Áudio</h2>
                        <p className="text-sm text-gray-400 mt-1">{streamTitle}</p>
                    </div>
                    <button 
                        onClick={onClose}
                        className="text-gray-400 hover:text-white transition"
                    >
                        <X size={24} />
                    </button>
                </div>

                {/* Conteúdo - Scrollable */}
                <div className="flex-1 overflow-y-auto px-6 py-4">
                    {loading ? (
                        <div className="text-center py-8 text-gray-400">Carregando...</div>
                    ) : (
                        <div className="space-y-6">
                        {/* Fonte de Áudio */}
                        <div>
                            <label className="block text-sm font-medium text-gray-300 mb-3">
                                Fonte de Áudio do Sistema
                            </label>
                            <div className="space-y-2">
                                <button
                                    onClick={() => handleAudioModeChange('system')}
                                    className={`w-full p-3 rounded-lg border transition flex items-center gap-3 ${
                                        config.audioMode === 'system' 
                                            ? 'border-blue-500 bg-blue-500/10 text-white' 
                                            : 'border-gray-600 bg-gray-700/50 text-gray-300 hover:border-gray-500'
                                    }`}
                                >
                                    <Volume2 size={20} />
                                    <div className="text-left">
                                        <div className="font-medium">Todo o Sistema</div>
                                        <div className="text-xs text-gray-400">Captura todos os sons do Windows</div>
                                    </div>
                                </button>

                                <button
                                    onClick={() => handleAudioModeChange('process')}
                                    className={`w-full p-3 rounded-lg border transition flex items-center gap-3 ${
                                        config.audioMode === 'process' 
                                            ? 'border-blue-500 bg-blue-500/10 text-white' 
                                            : 'border-gray-600 bg-gray-700/50 text-gray-300 hover:border-gray-500'
                                    }`}
                                >
                                    <Volume2 size={20} />
                                    <div className="text-left">
                                        <div className="font-medium">Aplicação Específica</div>
                                        <div className="text-xs text-gray-400">Selecione qual app capturar</div>
                                    </div>
                                </button>

                                <button
                                    onClick={() => handleAudioModeChange('muted')}
                                    className={`w-full p-3 rounded-lg border transition flex items-center gap-3 ${
                                        config.audioMode === 'muted' 
                                            ? 'border-red-500 bg-red-500/10 text-white' 
                                            : 'border-gray-600 bg-gray-700/50 text-gray-300 hover:border-gray-500'
                                    }`}
                                >
                                    <VolumeX size={20} />
                                    <div className="text-left">
                                        <div className="font-medium">Sem Áudio (Mudo)</div>
                                        <div className="text-xs text-gray-400">Não captura áudio do sistema</div>
                                    </div>
                                </button>
                            </div>
                        </div>

                        {/* Seletor de Processo */}
                        {config.audioMode === 'process' && (
                            <div>
                                <label className="block text-sm font-medium text-gray-300 mb-2">
                                    Selecione a Aplicação
                                </label>
                                {audioSessions.length > 0 ? (
                                    <Select 
                                        value={config.targetPid?.toString() || ''} 
                                        onChange={(e) => handleProcessSelect(e.target.value)}
                                        className="w-full"
                                    >
                                        <option value="">Escolha uma aplicação...</option>
                                        {audioSessions.map(session => (
                                            <option 
                                                key={session.process_id} 
                                                value={session.process_id.toString()}
                                            >
                                                {session.display_name || session.process_name} (PID: {session.process_id})
                                            </option>
                                        ))}
                                    </Select>
                                ) : (
                                    <div className="text-sm text-gray-400 p-3 bg-gray-700/50 rounded border border-gray-600">
                                        Nenhuma aplicação com áudio ativo detectada
                                    </div>
                                )}
                            </div>
                        )}

                        {/* Microfone */}
                        <div>
                            <label className="block text-sm font-medium text-gray-300 mb-2">
                                Microfone
                            </label>
                            <button
                                onClick={() => setConfig(prev => ({ ...prev, enableMicrophone: !prev.enableMicrophone }))}
                                className={`flex items-center gap-2 px-4 py-2 rounded-lg border transition ${
                                    config.enableMicrophone 
                                        ? 'border-green-500 bg-green-500/10 text-white' 
                                        : 'border-gray-600 bg-gray-700/50 text-gray-300'
                                }`}
                            >
                                {config.enableMicrophone ? <Mic size={18} /> : <MicOff size={18} />}
                                <span>{config.enableMicrophone ? 'Ativado' : 'Desativado'}</span>
                            </button>
                            {config.enableMicrophone && (
                                <p className="text-xs text-gray-400 mt-2">
                                    💡 Usando microfone padrão do Windows (WASAPI)
                                </p>
                            )}
                        </div>

                        {/* Divisor */}
                        <div className="border-t border-gray-700"></div>

                        {/* Audio Effects */}
                        <div>
                            <div className="flex items-center justify-between mb-3">
                                <label className="flex items-center gap-2 text-sm font-medium text-gray-300">
                                    <Sliders size={18} />
                                    Processamento de Áudio
                                </label>
                                <button
                                    onClick={() => setConfig(prev => ({
                                        ...prev,
                                        audioEffects: { ...prev.audioEffects, enabled: !prev.audioEffects.enabled }
                                    }))}
                                    className={`px-3 py-1 rounded-md text-xs font-medium transition ${
                                        config.audioEffects.enabled
                                            ? 'bg-blue-500 text-white'
                                            : 'bg-gray-700 text-gray-300'
                                    }`}
                                >
                                    {config.audioEffects.enabled ? 'Ativado' : 'Desativado'}
                                </button>
                            </div>

                            {config.audioEffects.enabled && (
                                <div className="space-y-4 pl-4 border-l-2 border-blue-500/30">
                                    {/* Sugestão de CPU */}
                                    {suggestedPreset && config.audioEffects.preset === 'cleanbalanced' && (
                                        <div className="bg-blue-500/10 border border-blue-500/30 rounded-lg p-3 text-xs">
                                            <div className="flex items-start gap-2">
                                                <Info size={16} className="text-blue-400 mt-0.5 flex-shrink-0" />
                                                <div>
                                                    <div className="text-blue-300 font-medium">💡 Sugestão baseada no seu hardware</div>
                                                    <div className="text-gray-300 mt-1">{suggestedPreset.reason}</div>
                                                    {suggestedPreset.preset !== config.audioEffects.preset && (
                                                        <button
                                                            onClick={() => setConfig(prev => ({
                                                                ...prev,
                                                                audioEffects: { ...prev.audioEffects, preset: suggestedPreset.preset }
                                                            }))}
                                                            className="mt-2 text-blue-400 hover:text-blue-300 underline"
                                                        >
                                                            Aplicar preset "{getPresetInfo(suggestedPreset.preset).name}"
                                                        </button>
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                    )}

                                    {/* Preset Selector */}
                                    <div>
                                        <label className="block text-sm font-medium text-gray-300 mb-2">
                                            Preset de Efeitos
                                        </label>
                                        <Select
                                            value={config.audioEffects.preset}
                                            onChange={(e) => setConfig(prev => ({
                                                ...prev,
                                                audioEffects: { ...prev.audioEffects, preset: e.target.value }
                                            }))}
                                            className="w-full"
                                        >
                                            <option value="none">🔇 Nenhum</option>
                                            <option value="cleanlight">✨ Limpo (Leve)</option>
                                            <option value="cleanbalanced">⚖️ Limpo (Balanceado) - Recomendado</option>
                                            <option value="professional">🎙️ Profissional</option>
                                            <option value="gaming">🎮 Gaming</option>
                                            <option value="podcast">🎧 Podcast</option>
                                            <option value="custom">⚙️ Personalizado</option>
                                        </Select>
                                        
                                        {/* Descrição do Preset */}
                                        <div className="mt-2 p-2 bg-gray-700/50 rounded text-xs text-gray-300">
                                            <div className="font-medium">{getPresetInfo(config.audioEffects.preset).description}</div>
                                            <div className="text-gray-400 mt-1">
                                                Impacto CPU: <span className="text-gray-300">{getPresetInfo(config.audioEffects.preset).cpuImpact}</span>
                                            </div>
                                        </div>
                                    </div>

                                    {/* Custom Filters */}
                                    {config.audioEffects.preset === 'custom' && (
                                        <div>
                                            <label className="block text-sm font-medium text-gray-300 mb-2">
                                                Filtros FFmpeg Personalizados
                                            </label>
                                            <textarea
                                                value={config.audioEffects.customFilters || ''}
                                                onChange={(e) => setConfig(prev => ({
                                                    ...prev,
                                                    audioEffects: { ...prev.audioEffects, customFilters: e.target.value }
                                                }))}
                                                placeholder="highpass=f=100,volume=1.5"
                                                className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono text-gray-200 resize-none"
                                                rows={3}
                                            />
                                            <a
                                                href="https://ffmpeg.org/ffmpeg-filters.html#Audio-Filters"
                                                target="_blank"
                                                rel="noopener noreferrer"
                                                className="text-xs text-blue-400 hover:text-blue-300 mt-1 inline-block"
                                            >
                                                📖 Ver documentação de filtros FFmpeg
                                            </a>
                                        </div>
                                    )}

                                    {/* Volume Master */}
                                    <div>
                                        <label className="block text-sm font-medium text-gray-300 mb-2">
                                            Volume: {Math.round(config.audioEffects.masterVolume * 100)}%
                                        </label>
                                        <input
                                            type="range"
                                            min="0"
                                            max="200"
                                            value={config.audioEffects.masterVolume * 100}
                                            onChange={(e) => setConfig(prev => ({
                                                ...prev,
                                                audioEffects: { ...prev.audioEffects, masterVolume: parseFloat(e.target.value) / 100 }
                                            }))}
                                            className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-blue-500"
                                        />
                                        <div className="flex justify-between text-xs text-gray-400 mt-1">
                                            <span>0%</span>
                                            <span>100%</span>
                                            <span>200%</span>
                                        </div>
                                    </div>

                                    {/* Warning para presets pesados */}
                                    {(config.audioEffects.preset === 'professional' || config.audioEffects.preset === 'podcast') && (
                                        <div className="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-3 text-xs">
                                            <div className="flex items-start gap-2">
                                                <Info size={16} className="text-yellow-400 mt-0.5 flex-shrink-0" />
                                                <div className="text-yellow-200">
                                                    ⚠️ Este preset usa processamento pesado de CPU ({getPresetInfo(config.audioEffects.preset).cpuImpact}). 
                                                    Recomendado apenas para PCs com CPU potente (i5/Ryzen 5 ou superior).
                                                </div>
                                            </div>
                                        </div>
                                    )}
                                </div>
                            )}
                        </div>

                        {/* Microphone Processing */}
                        {config.enableMicrophone && (
                            <>
                                <div className="border-t border-gray-700"></div>
                                
                                <div>
                                    <div className="flex items-center justify-between mb-3">
                                        <label className="flex items-center gap-2 text-sm font-medium text-gray-300">
                                            <Mic size={18} />
                                            Processamento de Microfone
                                        </label>
                                        <button
                                            onClick={() => setConfig(prev => ({
                                                ...prev,
                                                microphoneEffects: { 
                                                    ...prev.microphoneEffects, 
                                                    enabled: !prev.microphoneEffects?.enabled 
                                                }
                                            }))}
                                            className={`px-3 py-1 rounded-md text-xs font-medium transition ${
                                                config.microphoneEffects?.enabled
                                                    ? 'bg-green-500 text-white'
                                                    : 'bg-gray-700 text-gray-300'
                                            }`}
                                        >
                                            {config.microphoneEffects?.enabled ? 'Ativado' : 'Desativado'}
                                        </button>
                                    </div>

                                    {config.microphoneEffects?.enabled && (
                                        <div className="space-y-4 pl-4 border-l-2 border-green-500/30">
                                            {/* Preset Selector */}
                                            <div>
                                                <label className="block text-sm font-medium text-gray-300 mb-2">
                                                    Preset de Microfone
                                                </label>
                                                <Select
                                                    value={config.microphoneEffects?.preset || 'balanced'}
                                                    onChange={(e) => setConfig(prev => ({
                                                        ...prev,
                                                        microphoneEffects: { 
                                                            ...prev.microphoneEffects, 
                                                            preset: e.target.value 
                                                        }
                                                    }))}
                                                    className="w-full"
                                                >
                                                    {microphonePresets.length > 0 ? (
                                                        microphonePresets.map(preset => (
                                                            <option key={preset.id} value={preset.id}>
                                                                {preset.name}
                                                            </option>
                                                        ))
                                                    ) : (
                                                        <>
                                                            <option value="none">🔇 Nenhum</option>
                                                            <option value="light">✨ Leve</option>
                                                            <option value="balanced">⚖️ Balanceado - Recomendado</option>
                                                            <option value="podcast">🎙️ Podcast</option>
                                                            <option value="professional">🎤 Profissional</option>
                                                            <option value="custom">⚙️ Personalizado</option>
                                                        </>
                                                    )}
                                                </Select>
                                                
                                                {/* Descrição do Preset */}
                                                <div className="mt-2 p-2 bg-gray-700/50 rounded text-xs text-gray-300">
                                                    <div className="font-medium">{getMicPresetInfo(config.microphoneEffects?.preset || 'balanced').description}</div>
                                                    <div className="text-gray-400 mt-1">
                                                        Impacto CPU: <span className="text-gray-300">{getMicPresetInfo(config.microphoneEffects?.preset || 'balanced').cpuImpact}</span>
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Custom Filters */}
                                            {config.microphoneEffects?.preset === 'custom' && (
                                                <div>
                                                    <label className="block text-sm font-medium text-gray-300 mb-2">
                                                        Filtros FFmpeg Personalizados
                                                    </label>
                                                    <textarea
                                                        value={config.microphoneEffects?.customFilters || ''}
                                                        onChange={(e) => setConfig(prev => ({
                                                            ...prev,
                                                            microphoneEffects: { 
                                                                ...prev.microphoneEffects, 
                                                                customFilters: e.target.value 
                                                            }
                                                        }))}
                                                        placeholder="highpass=f=80,agate=threshold=0.01"
                                                        className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono text-gray-200 resize-none"
                                                        rows={3}
                                                    />
                                                    <a
                                                        href="https://ffmpeg.org/ffmpeg-filters.html#Audio-Filters"
                                                        target="_blank"
                                                        rel="noopener noreferrer"
                                                        className="text-xs text-blue-400 hover:text-blue-300 mt-1 inline-block"
                                                    >
                                                        📖 Ver documentação de filtros FFmpeg
                                                    </a>
                                                </div>
                                            )}

                                            {/* Volume do Microfone */}
                                            <div>
                                                <label className="block text-sm font-medium text-gray-300 mb-2">
                                                    Volume do Microfone: {Math.round((config.microphoneEffects?.volume || 1.0) * 100)}%
                                                </label>
                                                <input
                                                    type="range"
                                                    min="0"
                                                    max="200"
                                                    value={(config.microphoneEffects?.volume || 1.0) * 100}
                                                    onChange={(e) => setConfig(prev => ({
                                                        ...prev,
                                                        microphoneEffects: { 
                                                            ...prev.microphoneEffects, 
                                                            volume: parseFloat(e.target.value) / 100 
                                                        }
                                                    }))}
                                                    className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-green-500"
                                                />
                                                <div className="flex justify-between text-xs text-gray-400 mt-1">
                                                    <span>0%</span>
                                                    <span>100%</span>
                                                    <span>200%</span>
                                                </div>
                                            </div>

                                            {/* Warning para presets pesados */}
                                            {config.microphoneEffects?.preset === 'professional' && (
                                                <div className="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-3 text-xs">
                                                    <div className="flex items-start gap-2">
                                                        <Info size={16} className="text-yellow-400 mt-0.5 flex-shrink-0" />
                                                        <div className="text-yellow-200">
                                                            ⚠️ Preset profissional usa processamento completo incluindo de-esser. 
                                                            Recomendado para PCs com CPU potente.
                                                        </div>
                                                    </div>
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </div>
                            </>
                        )}
                    </div>
                )}
                </div>

                {/* Botões - Fixos no fundo */}
                {!loading && (
                    <div className="flex gap-3 p-6 pt-4 border-t border-gray-700">
                        <Button 
                            onClick={onClose} 
                            variant="outline" 
                            className="flex-1"
                            disabled={saving}
                        >
                            Cancelar
                        </Button>
                        <Button 
                            onClick={handleSave} 
                            className="flex-1 bg-blue-600 hover:bg-blue-700"
                            disabled={saving}
                        >
                            {saving ? 'Salvando...' : 'Salvar'}
                        </Button>
                    </div>
                )}
            </div>

            {/* AlertDialog para erros */}
            <AlertDialog open={!!errorMessage} onOpenChange={(open) => !open && setErrorMessage(null)}>
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>Erro</AlertDialogTitle>
                        <AlertDialogDescription>
                            {errorMessage}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogAction onClick={() => setErrorMessage(null)}>Ok</AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
