# 🎥 HipoCast

Sistema profissional de captura de tela e streaming via HLS com suporte a múltiplas fontes simultâneas, captura de áudio avançada e transmissão de links web.

## 📋 Descrição

HipoCast é uma aplicação desktop desenvolvida com Tauri + React que permite capturar janelas, monitores e páginas web, transmitindo-as via protocolo HLS (HTTP Live Streaming) com baixa latência. Ideal para streaming, gravação de gameplay, transmissão de apresentações e muito mais.

## ✨ Funcionalidades

### Captura de Vídeo
- 🖥️ **Captura de Janelas**: Capture qualquer janela do sistema
- 🌐 **Captura de Links Web**: Transmita páginas web em janela isolada
- 📺 **Suporte a Múltiplos Monitores**: Capture diferentes monitores simultaneamente
- 🎬 **Codificação H.264**: Vídeo otimizado com suporte a aceleração por hardware (NVENC, QuickSync, AMF)
- ⚡ **Baixa Latência**: Streaming HLS otimizado para transmissão em tempo real

### Captura de Áudio
- 🎵 **Áudio do Sistema**: Captura áudio via WASAPI (Windows Audio Session API)
- 🎙️ **Captura de Microfone**: Suporte a múltiplos dispositivos de entrada
- 🎯 **Captura por Processo**: Isole áudio de aplicações específicas
- 🎛️ **Presets de Efeitos**: Filtros de áudio profissionais (Podcast, Gaming, Professional)
- 🔊 **Controle de Volume**: Ajuste independente de áudio do sistema e microfone

### Gerenciamento de Streams
- 📁 **IDs Personalizados**: Defina identificadores customizados para suas streams
- ⏯️ **Pausar/Retomar**: Pause e retome streams mantendo configurações
- 💾 **Configurações Persistentes**: Salve configurações de áudio por stream
- 🔄 **Múltiplas Streams**: Gerencie várias capturas simultâneas
- 🗂️ **Organização Automática**: Segmentos HLS organizados por stream

### Interface
- 🎨 **UI Moderna**: Interface construída com React + Tailwind CSS
- 🌙 **Tema Escuro**: Design otimizado para longas sessões
- 📊 **Visualização em Tempo Real**: Preview das streams ativas
- ⚙️ **Configurações Avançadas**: Controle total sobre qualidade e performance

## 🚀 Instalação

### Pré-requisitos

- **Node.js** 18+ 
- **Rust** 1.70+
- **Windows** 10/11 (com suporte a Graphics Capture API)
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
3. Dê um nome para a stream
4. (Opcional) Defina um identificador personalizado
5. Clique em "Iniciar Captura"

### 2. Configurar Áudio

1. Clique no ícone de áudio (🔊) da stream
2. Escolha o modo de áudio:
   - **Sistema**: Captura todo áudio do sistema
   - **Processo**: Captura áudio de aplicação específica
   - **Mudo**: Sem áudio
3. Ative o microfone se necessário
4. Selecione presets de efeitos ou configure manualmente
5. Clique em "Salvar"

### 3. Acessar a Stream

A URL HLS é gerada automaticamente:
```
http://localhost:8080/streams/{stream_id}/{stream_id}.m3u8
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

### Áudio
- **Bitrate**: 128, 192, 256, 320 kbps
- **Modo**: Sistema, Processo Específico, Mudo
- **Presets**: CleanLight, CleanBalanced, Professional, Gaming, Podcast

### HLS
- **Duração do Segmento**: 2-6 segundos
- **Número de Segmentos**: 3-10

## 🏗️ Arquitetura

### Frontend (React + Vite)
- **UI**: React 18 + Tailwind CSS + shadcn/ui
- **Estado**: React Hooks
- **Comunicação**: Tauri IPC
- **Player**: HLS.js para reprodução

### Backend (Rust + Tauri)
- **Captura de Vídeo**: Windows Graphics Capture API
- **Captura de Áudio**: WASAPI (Windows Audio Session API)
- **Codificação**: FFmpeg com pipes nomeados
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
│   │   ├── audio.rs     # Captura de áudio WASAPI
│   │   ├── encoder.rs   # Codificação H.264/AAC
│   │   ├── ffmpeg.rs    # Integração FFmpeg
│   │   ├── hls.rs       # Servidor HLS
│   │   ├── db.rs        # Banco de dados
│   │   └── stream_config.rs  # Configurações
│   └── binaries/        # FFmpeg incluído
└── public/streams/      # Segmentos HLS gerados
```

## 🛠️ Tecnologias

- **Tauri** - Framework desktop
- **React** - UI Framework
- **Rust** - Backend de alta performance
- **FFmpeg** - Codificação de mídia
- **WASAPI** - Captura de áudio Windows
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
npm run lint
```

---

**Desenvolvido com ❤️ usando Tauri + React + Rust**
