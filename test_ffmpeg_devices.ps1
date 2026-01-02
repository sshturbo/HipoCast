# Script para testar FFmpeg DirectShow - Salva output em arquivo
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Testando FFmpeg DirectShow" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Caminho do FFmpeg no projeto
$ffmpegPath = ".\src-tauri\binaries\ffmpeg.exe"
$outputFile = ".\ffmpeg_devices_output.txt"

if (-not (Test-Path $ffmpegPath)) {
    Write-Host "ERRO: FFmpeg nao encontrado em: $ffmpegPath" -ForegroundColor Red
    exit 1
}

Write-Host "OK FFmpeg encontrado!" -ForegroundColor Green
Write-Host ""
Write-Host "Executando listagem de dispositivos..." -ForegroundColor Yellow

# Executar FFmpeg e salvar output em arquivo
& $ffmpegPath -list_devices true -f dshow -i dummy 2>&1 | Out-File -FilePath $outputFile -Encoding UTF8

Write-Host ""
Write-Host "Output salvo em: $outputFile" -ForegroundColor Green
Write-Host ""
Write-Host "Exibindo conteudo:" -ForegroundColor Cyan
Write-Host "==================" -ForegroundColor Cyan

# Ler e exibir o arquivo
Get-Content $outputFile

Write-Host ""
Write-Host "==================" -ForegroundColor Cyan
Write-Host "Teste concluido!" -ForegroundColor Green
