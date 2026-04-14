#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct AgentListBloomCompositeUniform {
    tint: vec4<f32>,
    occlusion_rects_uv: array<vec4<f32>, 4>,
    occlusion_rect_count: vec4<f32>,
}

@group(2) @binding(0) var input_texture: texture_2d<f32>;
@group(2) @binding(1) var input_sampler: sampler;
@group(2) @binding(2) var<uniform> material: AgentListBloomCompositeUniform;

fn occluded(uv: vec2<f32>) -> bool {
    let count = i32(material.occlusion_rect_count.x);
    for (var index = 0; index < 4; index = index + 1) {
        if (index >= count) {
            break;
        }
        let rect = material.occlusion_rects_uv[index];
        if rect.z > rect.x
            && rect.w > rect.y
            && uv.x >= rect.x
            && uv.x <= rect.z
            && uv.y >= rect.y
            && uv.y <= rect.w {
            return true;
        }
    }
    return false;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if occluded(in.uv) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let sample = textureSample(input_texture, input_sampler, in.uv);
    let rgb = sample.rgb * material.tint.rgb;
    return vec4<f32>(rgb, sample.a * material.tint.a);
}
