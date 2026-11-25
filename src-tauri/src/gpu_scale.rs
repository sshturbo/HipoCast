use std::sync::OnceLock;
use windows::core::{s, PCSTR};
use windows::Win32::Graphics::{
    Direct3D::{
        Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3},
        ID3DInclude,
    },
    Direct3D11::{
        ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
        ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_BIND_SHADER_RESOURCE,
        D3D11_BIND_UNORDERED_ACCESS, D3D11_TEXTURE2D_DESC,
    },
    Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
};

const THREADS_PER_AXIS: u32 = 8;

// Center-based nearest-neighbor sampling matches the CPU resize and keeps the RGBA8 channels.
const RESIZE_SHADER: &[u8] = br#"
Texture2D<float4> source : register(t0);
RWTexture2D<float4> destination : register(u0);

[numthreads(8, 8, 1)]
void main(uint3 threadId : SV_DispatchThreadID)
{
    uint2 inputSize, outputSize;
    source.GetDimensions(inputSize.x, inputSize.y);
    destination.GetDimensions(outputSize.x, outputSize.y);
    if (any(threadId.xy >= outputSize))
        return;

    uint2 samplePosition = ((threadId.xy * 2 + 1) * inputSize) / (outputSize * 2);
    destination[threadId.xy] = source.Load(int3(samplePosition, 0));
}
"#;

type ScaleError = Box<dyn std::error::Error + Send + Sync>;

pub(crate) struct GpuScaler {
    shader: ID3D11ComputeShader,
    textures: Option<ScaleTextures>,
}

struct ScaleTextures {
    input_size: (u32, u32),
    output_size: (u32, u32),
    input: ID3D11Texture2D,
    output: ID3D11Texture2D,
    input_view: Option<ID3D11ShaderResourceView>,
    output_view: Option<ID3D11UnorderedAccessView>,
}

impl GpuScaler {
    pub(crate) fn new(device: &ID3D11Device) -> Result<Self, ScaleError> {
        static BYTECODE: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();
        let bytecode = BYTECODE
            .get_or_init(|| compile_shader().map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| error.clone())?;
        let mut shader = None;
        // The bytecode and output slot are valid throughout this call.
        unsafe { device.CreateComputeShader(bytecode, None, Some(&mut shader))? };
        Ok(Self {
            shader: shader.ok_or("Direct3D não criou o shader de escala.")?,
            textures: None,
        })
    }

    pub(crate) fn resize(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        input_size: (u32, u32),
        output_size: (u32, u32),
    ) -> Result<ID3D11Texture2D, ScaleError> {
        if self.textures.as_ref().is_none_or(|textures| {
            textures.input_size != input_size || textures.output_size != output_size
        }) {
            self.textures = Some(ScaleTextures::new(device, input_size, output_size)?);
        }
        let textures = self.textures.as_ref().unwrap();

        // The capture frame and both cached textures belong to this device and have matching
        // RGBA8 format and mip/array counts. The capture thread owns the immediate context here.
        unsafe {
            context.CopyResource(&textures.input, source);
            context.CSSetShader(&self.shader, None);
            context.CSSetShaderResources(0, Some(std::slice::from_ref(&textures.input_view)));
            context.CSSetUnorderedAccessViews(0, 1, Some(&textures.output_view), None);
            context.Dispatch(
                output_size.0.div_ceil(THREADS_PER_AXIS),
                output_size.1.div_ceil(THREADS_PER_AXIS),
                1,
            );
            context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            context.CSSetShaderResources(0, Some(&[None]));
            context.CSSetShader(None, None);
        }
        Ok(textures.output.clone())
    }
}

impl ScaleTextures {
    fn new(
        device: &ID3D11Device,
        input_size: (u32, u32),
        output_size: (u32, u32),
    ) -> Result<Self, ScaleError> {
        let input = new_texture(device, input_size, D3D11_BIND_SHADER_RESOURCE.0 as u32)?;
        let output = new_texture(device, output_size, D3D11_BIND_UNORDERED_ACCESS.0 as u32)?;
        let (mut input_view, mut output_view) = (None, None);
        // Each texture has the bind flags required by its corresponding view.
        unsafe {
            device.CreateShaderResourceView(&input, None, Some(&mut input_view))?;
            device.CreateUnorderedAccessView(&output, None, Some(&mut output_view))?;
        }
        if input_view.is_none() || output_view.is_none() {
            return Err("Direct3D não criou as views para escala.".into());
        }
        Ok(Self {
            input_size,
            output_size,
            input,
            output,
            input_view,
            output_view,
        })
    }
}

fn new_texture(
    device: &ID3D11Device,
    size: (u32, u32),
    bind_flags: u32,
) -> Result<ID3D11Texture2D, ScaleError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: bind_flags,
        ..Default::default()
    };
    let mut texture = None;
    // The description and output slot are valid throughout this call.
    unsafe { device.CreateTexture2D(&description, None, Some(&mut texture))? };
    texture.ok_or_else(|| "Direct3D não criou a textura para escala.".into())
}

fn compile_shader() -> Result<Vec<u8>, ScaleError> {
    let mut blob = None;
    // The shader source is a static byte slice and all borrowed inputs outlive the call.
    unsafe {
        D3DCompile(
            RESIZE_SHADER.as_ptr().cast(),
            RESIZE_SHADER.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            s!("main"),
            s!("cs_5_0"),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut blob,
            None,
        )?;
    }
    let blob = blob.ok_or("A compilação do shader Direct3D não retornou bytecode.")?;
    // The blob owns initialized bytecode of this size until it is copied below.
    Ok(unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()).to_vec()
    })
}
