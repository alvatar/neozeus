#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct HudBloomMaterial {
    texel_step: vec2<f32>,
    direction: vec2<f32>,
    intensity: f32,
    _padding: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: HudBloomMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var source_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var source_sampler: sampler;

fn sample_blur(uv: vec2<f32>) -> vec4<f32> {
    let step = material.texel_step * material.direction;
    var color = textureSample(source_texture, source_sampler, uv) * 0.22702703;
    color += textureSample(source_texture, source_sampler, uv + step * 1.3846154) * 0.31621623;
    color += textureSample(source_texture, source_sampler, uv - step * 1.3846154) * 0.31621623;
    color += textureSample(source_texture, source_sampler, uv + step * 3.2307692) * 0.07027027;
    color += textureSample(source_texture, source_sampler, uv - step * 3.2307692) * 0.07027027;
    return color;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = sample_blur(mesh.uv);
    return vec4<f32>(blurred.rgb * material.intensity, blurred.a * material.intensity);
}
