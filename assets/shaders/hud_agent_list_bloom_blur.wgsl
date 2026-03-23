#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct AgentListBloomBlurUniform {
    texel_step_gain: vec4<f32>,
}

@group(2) @binding(0) var input_texture: texture_2d<f32>;
@group(2) @binding(1) var input_sampler: sampler;
@group(2) @binding(2) var<uniform> material: AgentListBloomBlurUniform;

fn sample_offset(uv: vec2<f32>, direction: vec2<f32>, radius_pixels: f32, radius_scale: f32) -> vec3<f32> {
    let offset = direction * material.texel_step_gain.xy * radius_pixels * radius_scale;
    return textureSample(input_texture, input_sampler, uv + offset).rgb;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius_pixels = material.texel_step_gain.z;
    let gain = material.texel_step_gain.w;

    var glow = textureSample(input_texture, input_sampler, in.uv).rgb * 0.18;

    let dir0 = vec2<f32>(1.0, 0.0);
    let dir1 = vec2<f32>(0.0, 1.0);
    let dir2 = normalize(vec2<f32>(1.0, 1.0));
    let dir3 = normalize(vec2<f32>(1.0, -1.0));

    let ring1_weight = 0.12;
    glow += sample_offset(in.uv, dir0, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, -dir0, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, dir1, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, -dir1, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, dir2, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, -dir2, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, dir3, radius_pixels, 0.45) * ring1_weight;
    glow += sample_offset(in.uv, -dir3, radius_pixels, 0.45) * ring1_weight;

    let ring2_weight = 0.04;
    glow += sample_offset(in.uv, dir0, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, -dir0, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, dir1, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, -dir1, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, dir2, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, -dir2, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, dir3, radius_pixels, 1.0) * ring2_weight;
    glow += sample_offset(in.uv, -dir3, radius_pixels, 1.0) * ring2_weight;

    glow *= gain;
    let alpha = clamp(max(max(glow.r, glow.g), glow.b), 0.0, 1.0);
    return vec4<f32>(glow, alpha);
}
