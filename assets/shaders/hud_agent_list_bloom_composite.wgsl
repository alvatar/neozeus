#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct AgentListBloomCompositeUniform {
    core_center_size: vec4<f32>,
    settings: vec4<f32>,
}

@group(2) @binding(0) var bloom_texture: texture_2d<f32>;
@group(2) @binding(1) var bloom_sampler: sampler;
@group(2) @binding(2) var<uniform> material: AgentListBloomCompositeUniform;

fn rect_signed_distance(point: vec2<f32>, center: vec2<f32>, size: vec2<f32>) -> f32 {
    let half_size = size * 0.5;
    let q = abs(point - center) - half_size;
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(bloom_texture, bloom_sampler, in.uv);
    let signed_distance = rect_signed_distance(
        in.world_position.xy,
        material.core_center_size.xy,
        material.core_center_size.zw,
    );
    let halo_mask = smoothstep(0.0, material.settings.y, signed_distance);
    return vec4<f32>(sampled.rgb * halo_mask, sampled.a * material.settings.x * halo_mask);
}
