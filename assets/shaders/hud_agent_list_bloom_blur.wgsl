#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct AgentListBloomBlurUniform {
    texel_step_gain: vec4<f32>,
}

@group(2) @binding(0) var input_texture: texture_2d<f32>;
@group(2) @binding(1) var input_sampler: sampler;
@group(2) @binding(2) var<uniform> material: AgentListBloomBlurUniform;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let step_uv = material.texel_step_gain.xy;
    let gain = material.texel_step_gain.z;

    var color = textureSample(input_texture, input_sampler, in.uv) * 0.227027;
    color += textureSample(input_texture, input_sampler, in.uv + step_uv * 1.384615) * 0.316216;
    color += textureSample(input_texture, input_sampler, in.uv - step_uv * 1.384615) * 0.316216;
    color += textureSample(input_texture, input_sampler, in.uv + step_uv * 3.230769) * 0.070270;
    color += textureSample(input_texture, input_sampler, in.uv - step_uv * 3.230769) * 0.070270;

    let glow = color.rgb * gain;
    let alpha = clamp(max(max(glow.r, glow.g), glow.b), 0.0, 1.0);
    return vec4<f32>(glow, alpha);
}
