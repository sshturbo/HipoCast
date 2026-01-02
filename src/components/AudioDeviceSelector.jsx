import React, { useState, useEffect } from 'react';
import { listAudioDevices } from '../api/streams';

/**
 * Componente para seleção de dispositivos de áudio
 * Permite usuário escolher dispositivo de sistema (loopback) e microfone
 * @param {string} deviceType - "loopback" ou "input" ou undefined (mostra ambos)
 */
export function AudioDeviceSelector({ settings, onUpdate, deviceType }) {
    const [devices, setDevices] = useState([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState(null);

    // Carregar dispositivos ao montar componente
    useEffect(() => {
        loadDevices();
    }, []);

    const loadDevices = async (force = false) => {
        setLoading(true);
        setError(null);
        try {
            const isForce = typeof force === 'boolean' ? force : false;
            const deviceList = await listAudioDevices(isForce);
            setDevices(deviceList);
            console.log('📱 Dispositivos carregados:', deviceList);
        } catch (err) {
            setError('Falha ao carregar dispositivos de áudio');
            console.error('Erro ao carregar dispositivos:', err);
        } finally {
            setLoading(false);
        }
    };

    // Filtrar dispositivos por tipo
    const loopbackDevices = devices.filter(d => d.deviceType === 'loopback');
    const inputDevices = devices.filter(d => d.deviceType === 'input');

    // Se deviceType for especificado, mostrar apenas esse tipo
    const showLoopback = !deviceType || deviceType === 'loopback';
    const showInput = !deviceType || deviceType === 'input';

    return (
        <div className="audio-device-selector-inline">
            {error && (
                <div className="error-message">
                    ⚠️ {error}
                </div>
            )}

            <div className="device-selectors">
                {/* Dispositivo de Áudio do Sistema (Loopback) */}
                {showLoopback && (
                    <div className="selector-group">
                        <div className="flex justify-between items-center mb-2">
                            <label htmlFor="audio-device" className="text-sm font-medium text-slate-300">
                                🔊 Dispositivo de Loopback
                            </label>
                            <button
                                onClick={() => loadDevices(true)}
                                disabled={loading}
                                className="btn-refresh-small"
                            >
                                {loading ? '⏳' : '🔄'}
                            </button>
                        </div>
                        <select
                            id="audio-device"
                            value={settings.audioDevice || ''}
                            onChange={(e) => onUpdate({ ...settings, audioDevice: e.target.value })}
                            disabled={loading || loopbackDevices.length === 0}
                            className="w-full px-3 py-2 bg-slate-900 border border-slate-700 rounded-lg text-sm text-slate-300"
                        >
                            <option value="">Selecione um dispositivo</option>
                            {loopbackDevices.map((device, idx) => (
                                <option key={idx} value={device.name}>
                                    {device.name}
                                </option>
                            ))}
                        </select>
                        {loopbackDevices.length === 0 && !loading && (
                            <small className="hint">
                                ⚠️ Nenhum dispositivo de loopback encontrado.
                                Habilite "Stereo Mix" nas configurações de áudio do Windows.
                            </small>
                        )}
                    </div>
                )}

                {/* Dispositivo de Microfone */}
                {showInput && (
                    <div className="selector-group">
                        <div className="flex justify-between items-center mb-2">
                            <label htmlFor="microphone-device" className="text-sm font-medium text-slate-300">
                                🎤 Dispositivo de Entrada
                            </label>
                            <button
                                onClick={() => loadDevices(true)}
                                disabled={loading}
                                className="btn-refresh-small"
                            >
                                {loading ? '⏳' : '🔄'}
                            </button>
                        </div>
                        <select
                            id="microphone-device"
                            value={settings.microphoneDevice || ''}
                            onChange={(e) => onUpdate({ ...settings, microphoneDevice: e.target.value })}
                            disabled={loading || inputDevices.length === 0}
                            className="w-full px-3 py-2 bg-slate-900 border border-slate-700 rounded-lg text-sm text-slate-300"
                        >
                            <option value="">Selecione um dispositivo</option>
                            {inputDevices.map((device, idx) => (
                                <option key={idx} value={device.name}>
                                    {device.name}
                                </option>
                            ))}
                        </select>
                        {inputDevices.length === 0 && !loading && (
                            <small className="hint">
                                ⚠️ Nenhum microfone encontrado.
                            </small>
                        )}
                    </div>
                )}
            </div>

            <style jsx>{`
                .audio-device-selector-inline {
                    width: 100%;
                }

                .btn-refresh-small {
                    padding: 4px 8px;
                    background: transparent;
                    color: #94a3b8;
                    border: 1px solid #334155;
                    border-radius: 6px;
                    cursor: pointer;
                    font-size: 14px;
                    transition: all 0.2s;
                }

                .btn-refresh-small:hover:not(:disabled) {
                    background: #1e293b;
                    color: #e2e8f0;
                    border-color: #475569;
                }

                .btn-refresh-small:disabled {
                    opacity: 0.5;
                    cursor: not-allowed;
                }

                .error-message {
                    padding: 8px 12px;
                    background: rgba(239, 68, 68, 0.1);
                    border: 1px solid rgba(239, 68, 68, 0.2);
                    border-radius: 6px;
                    color: #fca5a5;
                    margin-bottom: 12px;
                    font-size: 12px;
                }

                .device-selectors {
                    display: grid;
                    gap: 16px;
                }

                .selector-group {
                    display: flex;
                    flex-direction: column;
                    gap: 6px;
                }

                .hint {
                    color: #64748b;
                    font-size: 11px;
                    font-style: italic;
                    margin-top: 4px;
                    display: block;
                }
            `}</style>
        </div>
    );
}
