// Placeholder function to list audio devices using FFmpeg DirectShow
// This will be implemented to call: ffmpeg -list_devices true -f dshow -i dummy
#[tauri::command]
fn list_audio_devices_ffmpeg() -> Result<Vec<String>, String> {
    // TODO: Implementar listagem via FFmpeg DirectShow
    // Comando: ffmpeg -list_devices true -f dshow -i dummy
    // Parsear output para extrair nomes de dispositivos

    println!("⚠️ list_audio_devices_ffmpeg not yet implemented");
    Ok(vec![
        "Stereo Mix".to_string(),
        "Microphone (Realtek High Definition Audio)".to_string(),
    ])
}
