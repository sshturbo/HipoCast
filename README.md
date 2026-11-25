# 🎥 HipoCast

Aplicativo Windows para transmitir uma janela, um monitor ou uma página web via HLS. O HipoCast permite salvar várias transmissões e manter uma captura ativa por vez.

## 📋 Descrição

HipoCast é uma aplicação desktop desenvolvida com Tauri + React que permite capturar janelas, monitores e páginas web, transmitindo-as via protocolo HLS (HTTP Live Streaming) com baixa latência. Ideal para streaming, gravação de gameplay, transmissão de apresentações e muito mais.

## ✨ Funcionalidades

### Captura de Vídeo
- 🖥️ **Captura de Janelas**: Capture qualquer janela do sistema
- 🌐 **Captura de Links Web**: Transmita páginas web em janela isolada
- 📺 **Suporte a Múltiplos Monitores**: Escolha qual monitor capturar
- 🎬 **Codificação H.264**: Vídeo otimizado com suporte a aceleração por hardware (NVENC, QuickSync, AMF)
- ⚡ **Baixa Latência**: Streaming HLS otimizado para transmissão em tempo real

### Captura de Áudio
- 🎵 **Áudio do Sistema**: Captura da saída padrão do Windows via WASAPI, com suporte a dispositivos de loopback DirectShow
- 🎙️ **Captura de Microfone**: Suporte a múltiplos dispositivos de entrada
- 🎛️ **Presets de Efeitos**: Filtros de áudio profissionais (Podcast, Gaming, Professional)
- 🔊 **Controle de Volume**: Ajuste independente de áudio do sistema e microfone

### Gerenciamento de Streams
- 📁 **IDs Personalizados**: Defina identificadores customizados para suas streams
- ⏯️ **Pausar/Retomar**: Pause e retome streams mantendo configurações
- 💾 **Configurações Persistentes**: Salve configurações de áudio por stream
- 🔄 **Transmissões salvas**: Alterne entre fontes usando Pausar e Retomar
- 🗂️ **Organização Automática**: Segmentos HLS organizados por stream

### Interface
- 🎨 **UI Moderna**: Interface construída com React + Tailwind CSS
- 🌙 **Tema Escuro**: Design otimizado para longas sessões
- 📊 **Visualização em Tempo Real**: Preview das streams ativas
- ⚙️ **Configurações Avançadas**: Controle total sobre qualidade e performance

## 🚀 Instalação

### Pré-requisitos

- **Node.js** 18+ 
- **Rust** stable com a toolchain MSVC
- **Windows** 10/11 (com suporte a Graphics Capture API)
- **Visual Studio Build Tools** com ferramentas de compilação C++ e **WebView2 Runtime**
- **FFmpeg** (incluído no binário)

### Desenvolvimento

1. Clone o repositório:
```bash
git clone https://github.com/seu-usuario/HipoCast.git
cd HipoCast
```

2. Instale as dependências:
```bash
npm install
```

3. Execute em modo desenvolvimento:
```bash
npm run tauri dev
```

### Build de Produção

```bash
npm run tauri build
```

O instalador será gerado em `src-tauri/target/release/bundle/`

## 🎮 Como Usar

### 1. Iniciar uma Captura

**Captura de Janela:**
1. Clique em "Selecionar Fonte"
2. Escolha a janela desejada
3. (Opcional) Defina um identificador personalizado
4. Clique em "Iniciar Captura"

**Captura de Link Web:**
1. Clique em "Link"
2. Cole a URL desejada
3. (Opcional) Defina um identificador personalizado
4. Clique em "Iniciar Captura"

### 2. Configurar Áudio

1. Para capturar áudio do sistema, habilite essa opção em Configurações. A saída padrão do Windows é usada automaticamente quando nenhuma outra saída de loopback disponível está selecionada.
2. Em Ajustes da transmissão, escolha **Todo o Sistema**, **Somente microfone** ou **Mudo**. O modo mudo desativa as duas entradas.
3. No modo **Todo o Sistema**, use **Incluir microfone** para combinar as duas entradas. Selecione o dispositivo de entrada sempre que usar o microfone.
4. Ajuste os efeitos e clique em "Salvar". A transmissão ativa reinicia para aplicar os ajustes.

O áudio por aplicação está indisponível. Se a saída padrão estiver indisponível, o vídeo pode iniciar com o microfone ou em silêncio. O aviso aparece no card da transmissão.

### 3. Acessar a Stream

A URL HLS é gerada automaticamente:
```
http://127.0.0.1:8080/streams/{stream_id}/{stream_id}.m3u8
```

Use em players compatíveis:
- **VLC Media Player**
- **OBS Studio** (Media Source)
- **Web Players** (hls.js, video.js)

### 4. Pausar/Retomar

- **Pausar**: Clique no botão ⏸️ - a janela permanece aberta
- **Retomar**: Clique no botão ▶️ - mantém todas as configurações

### 5. Remover Stream

- Clique no botão 🗑️ para parar, fechar a janela e remover os arquivos

## ⚙️ Configurações

### Vídeo
- **Resolução**: 1920x1080, 1280x720, 2560x1440, 3840x2160
- **Taxa de Quadros**: 30, 60 FPS
- **Bitrate**: 2500-10000 kbps
- **Aceleração de Hardware**: NVENC, QuickSync, AMF

O modo leve aplica 1280 × 720, 30 FPS e 2 Mbps com aceleração por hardware quando disponível. Em Configurações, clique em **Usar modo leve** e depois em **Salvar**. Desmarque **Prévia ao vivo** na tela de transmissão para suspender a reprodução dentro do HipoCast.

### Áudio
- **Bitrate**: 128, 192, 256, 320 kbps
- **Modo** — Áudio do sistema com microfone opcional, somente microfone ou mudo
- **Presets**: CleanLight, CleanBalanced, Professional, Gaming, Podcast

### HLS
- **Duração do Segmento**: 2-6 segundos
- **Segmentos na playlist** — Pelo menos 3. Digite `0` para manter todos os segmentos da transmissão em disco.

## 🏗️ Arquitetura

### Frontend (React + Vite)
- **UI**: React 18 + Tailwind CSS + shadcn/ui
- **Estado**: React Hooks
- **Comunicação**: Tauri IPC
- **Player**: HLS.js para reprodução

### Backend (Rust + Tauri)
- **Captura de Vídeo**: Windows Graphics Capture API
- **Captura de Áudio**: WASAPI para a saída padrão, DirectShow para o microfone e dispositivos de loopback compatíveis
- **Codificação**: FFmpeg recebe os frames via pipe
- **Servidor HLS**: Axum (HTTP server)
- **Banco de Dados**: SQLite

### Estrutura de Pastas
```
HipoCast/
├── src/                    # Frontend React
│   ├── components/        # Componentes UI
│   ├── api/              # Comunicação Tauri
│   └── assets/           # Recursos estáticos
├── src-tauri/            # Backend Rust
│   ├── src/
│   │   ├── capture.rs   # Captura de vídeo
│   │   ├── commands/   # Comandos Tauri e configuração de áudio
│   │   ├── encoder.rs   # Codificação H.264/AAC
│   │   ├── ffmpeg.rs    # Integração FFmpeg
│   │   ├── server.rs    # Servidor HLS
│   │   ├── db.rs        # Banco de dados
│   │   └── stream_config.rs  # Configurações
│   └── binaries/        # FFmpeg incluído
└── public/              # Arquivos estáticos
```

## 🛠️ Tecnologias

- **Tauri** - Framework desktop
- **React** - UI Framework
- **Rust** - Backend de alta performance
- **FFmpeg** - Codificação de mídia
- **WASAPI e DirectShow** - Captura de áudio no Windows
- **Graphics Capture API** - Captura de tela Windows
- **Axum** - Servidor HTTP
- **SQLite** - Banco de dados local
- **HLS.js** - Player de vídeo
- **Tailwind CSS** - Estilização

## 📝 Licença

Este projeto está sob a licença MIT. Veja o arquivo [LICENSE](LICENSE) para mais detalhes.

## 🤝 Contribuindo

Contribuições são bem-vindas! Sinta-se à vontade para abrir issues ou pull requests.

## 💡 Suporte

Para problemas, dúvidas ou sugestões, abra uma [issue](https://github.com/seu-usuario/HipoCast/issues) no GitHub.

## 🔧 Desenvolvimento

O banco e os segmentos HLS são gravados em `%LOCALAPPDATA%\com.fivey.hipocast`. O modo de desenvolvimento usa a subpasta `dev`. Na primeira execução, o app migra as configurações e as transmissões salvas do banco antigo, se ele existir.

### IDE Recomendado

- [VS Code](https://code.visualstudio.com/)
- [Tauri Extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### Comandos Úteis

```bash
# Desenvolvimento
npm run tauri dev

# Build de produção
npm run tauri build

# Compilar apenas Rust
cargo build --manifest-path src-tauri/Cargo.toml

# Linting
cargo clippy --manifest-path src-tauri/Cargo.toml
```

---

**Desenvolvido com ❤️ usando Tauri + React + Rust**
