import { invoke } from '@tauri-apps/api/core';

/**
 * Busca lista de janelas ativas no sistema via Tauri
 */
export async function fetchWindows() {
    try {
        const sources = await invoke('get_capture_sources');
        // Map to expected format - frontend expects lowercase field names
        return sources.map(s => ({
            id: s.id,
            title: s.title,
            type: s.source_type
        }));
    } catch (error) {
        console.error('Erro ao buscar janelas:', error);
        return [];
    }
}

/**
 * Inicia captura de uma janela via Tauri
 */
export async function startStream(sourceId, title = '') {
    try {
        const response = await invoke('start_source_capture', { id: sourceId, title });
        return response;
    } catch (error) {
        console.error('Erro ao iniciar stream:', error);
        throw error;
    }
}

/**
 * Inicia captura de um link (navegador isolado) via Tauri
 */
export async function startBrowserStream(url, title = 'Web Stream', customId = null) {
    try {
        const params = { url, title };
        // Só adicionar customId se for válido
        if (customId && customId.trim()) {
            params.customId = customId.trim();
        }
        const response = await invoke('start_browser_stream', params);
        return response;
    } catch (error) {
        console.error('Erro ao iniciar stream de link:', error);
        throw error;
    }
}

/**
 * Para uma captura ativa via Tauri
 */
export async function stopStream(id) {
    try {
        return await invoke('stop_source_capture');
    } catch (error) {
        console.error('Erro ao parar stream:', error);
        throw error;
    }
}

/**
 * Encerra uma captura e remove os arquivos
 */
export async function removeStream(id) {
    try {
        // Chama comando dedicado para remover do banco
        return await invoke('remove_stream', { id });
    } catch (error) {
        console.error('Erro ao remover stream:', error);
        throw error;
    }
}

/**
 * Busca status de todos os streams via Tauri
 */
export async function getStatus() {
    try {
        return await invoke('get_status');
    } catch (error) {
        console.error('Erro ao buscar status:', error);
        return { streams: [], active: 0 };
    }
}


/**
 * Busca configurações globais via Tauri
 */
export async function fetchSettings() {
    try {
        return await invoke('get_settings');
    } catch (error) {
        console.error('Erro ao buscar configurações:', error);
        return null;
    }
}

/**
 * Salva configurações globais via Tauri
 */
export async function updateSettings(settings) {
    try {
        return await invoke('update_settings', { newSettings: settings });
    } catch (error) {
        console.error('Erro ao salvar configurações:', error);
        throw error;
    }
}

/**
 * Lista dispositivos de áudio via FFmpeg DirectShow
 * Retorna: [{ name: string, deviceType: "loopback" | "input" }]
 */
export async function listAudioDevices() {
    try {
        return await invoke('list_audio_devices_ffmpeg');
    } catch (error) {
        console.error('Erro ao listar dispositivos de áudio:', error);
        return [];
    }
}

/**
 * Lista aplicações ativas com áudio (Audio Sessions)
 * NOTA: Função desabilitada após refatoração para FFmpeg DirectShow
 * Retorna: [{ process_id, process_name, display_name, icon_path }]
 */
export async function listAudioSessions() {
    try {
        // Função removida após refatoração - retorna array vazio
        console.warn('listAudioSessions foi removida após refatoração para FFmpeg DirectShow');
        return [];
    } catch (error) {
        console.error('Erro ao listar audio sessions:', error);
        return [];
    }
}

/**
 * Obtém configuração de áudio de uma stream específica
 */
export async function getStreamAudioConfig(streamId) {
    try {
        const config = await invoke('get_stream_audio_config', { streamId });
        if (!config) {
            return {
                streamId,
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
            };
        }
        return config;
    } catch (error) {
        // Se houver erro na chamada, retorna default
        return {
            streamId,
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
        };
    }
}

/**
 * Salva configuração de áudio de uma stream
 */
export async function saveStreamAudioConfig(config) {
    try {
        await invoke('save_stream_audio_config', { config });
    } catch (error) {
        console.error('Erro ao salvar config de áudio:', error);
        throw error;
    }
}

/**
 * Remove configuração de áudio de uma stream
 */
export async function deleteStreamAudioConfig(streamId) {
    try {
        await invoke('delete_stream_audio_config', { streamId });
    } catch (error) {
        console.error('Erro ao deletar config de áudio:', error);
    }
}
