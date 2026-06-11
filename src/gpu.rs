//! GPU-resident chunk rendering. macroquad's `draw_mesh` re-uploads every
//! vertex through its streaming batch each frame; chunk geometry is static,
//! so we keep it in immutable GPU buffers and issue raw miniquad draw calls
//! against the same shader. Dynamic geometry (entities, particles, UI) stays
//! on macroquad's batcher.

use macroquad::miniquad::*;
use macroquad::models::Vertex;

pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec4 color;
varying lowp vec2 uv;
varying lowp float torch;
varying lowp float sky;
varying lowp float shimmer;
varying highp vec3 wpos;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    torch = normal.x;
    sky = normal.y;
    shimmer = normal.z;
    wpos = position;
}"#;

pub const FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
varying lowp float torch;
varying lowp float sky;
varying lowp float shimmer;
varying highp vec3 wpos;
uniform sampler2D Texture;
uniform lowp vec4 daylight;
uniform highp vec4 fog;
uniform highp vec4 campos;
void main() {
    lowp vec4 c = color * texture2D(Texture, uv);
    lowp vec3 light = max(daylight.rgb * sky, vec3(torch * 0.95));
    light = max(light, vec3(0.04));
    if (shimmer > 0.5) {
        light *= 1.0 + 0.09 * sin(campos.w * 2.4 + wpos.x * 1.9 + wpos.z * 1.4);
    }
    lowp vec3 lit = c.rgb * light;
    highp float fd = distance(wpos, campos.xyz);
    highp float f = clamp((fd - fog.a * 0.72) / (fog.a * 0.28), 0.0, 1.0);
    gl_FragColor = vec4(mix(lit, fog.rgb, f), c.a);
}"#;

#[repr(C)]
pub struct Uniforms {
    pub projection: [f32; 16],
    pub daylight: [f32; 4],
    pub fog: [f32; 4],
    pub campos: [f32; 4],
}

pub struct GpuRenderer {
    pub pipeline: Pipeline,
}

impl GpuRenderer {
    pub fn new(ctx: &mut dyn RenderingBackend) -> GpuRenderer {
        let shader = ctx
            .new_shader(
                ShaderSource::Glsl {
                    vertex: VERTEX,
                    fragment: FRAGMENT,
                },
                ShaderMeta {
                    images: vec!["Texture".to_string()],
                    uniforms: UniformBlockLayout {
                        uniforms: vec![
                            UniformDesc::new("Projection", UniformType::Mat4),
                            UniformDesc::new("daylight", UniformType::Float4),
                            UniformDesc::new("fog", UniformType::Float4),
                            UniformDesc::new("campos", UniformType::Float4),
                        ],
                    },
                },
            )
            .expect("chunk shader compiles");
        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[
                VertexAttribute::new("position", VertexFormat::Float3),
                VertexAttribute::new("texcoord", VertexFormat::Float2),
                VertexAttribute::new("color0", VertexFormat::Byte4),
                VertexAttribute::new("normal", VertexFormat::Float4),
            ],
            shader,
            PipelineParams {
                depth_test: Comparison::LessOrEqual,
                depth_write: true,
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
        );
        GpuRenderer { pipeline }
    }
}

/// A static chunk mesh living entirely on the GPU.
pub struct GpuMesh {
    pub bindings: Bindings,
    pub num_indices: i32,
}

impl GpuMesh {
    pub fn new(
        ctx: &mut dyn RenderingBackend,
        vertices: &[Vertex],
        indices: &[u16],
        texture: TextureId,
    ) -> GpuMesh {
        let vb = ctx.new_buffer(
            BufferType::VertexBuffer,
            BufferUsage::Immutable,
            BufferSource::slice(vertices),
        );
        let ib = ctx.new_buffer(
            BufferType::IndexBuffer,
            BufferUsage::Immutable,
            BufferSource::slice(indices),
        );
        GpuMesh {
            bindings: Bindings {
                vertex_buffers: vec![vb],
                index_buffer: ib,
                images: vec![texture],
            },
            num_indices: indices.len() as i32,
        }
    }

    /// Free the GPU buffers (must be called before dropping).
    pub fn delete(&self, ctx: &mut dyn RenderingBackend) {
        for vb in &self.bindings.vertex_buffers {
            ctx.delete_buffer(*vb);
        }
        ctx.delete_buffer(self.bindings.index_buffer);
    }
}

/// One chunk column's GPU geometry.
pub struct ChunkGpu {
    pub opaque: Vec<GpuMesh>,
    pub transparent: Vec<GpuMesh>,
}

impl ChunkGpu {
    pub fn delete(&self, ctx: &mut dyn RenderingBackend) {
        for m in self.opaque.iter().chain(&self.transparent) {
            m.delete(ctx);
        }
    }
}
