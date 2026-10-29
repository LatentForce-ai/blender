// Solid shading for opaque meshes with studio lighting: a single forward pass equivalent to
// workbench's default prepass + resolve (draw/engines/workbench/shaders/). The lighting is a
// port of `get_world_lighting` in workbench_world_light.bsl.hh.

struct ViewData {
    view_projection: mat4x4<f32>,
    view: mat4x4<f32>,
}

struct LightData {
    direction: vec4<f32>,
    specular_color: vec4<f32>,
    // rgb: diffuse colour, a: wrap (`SolidLight::smooth`).
    diffuse_color_wrap: vec4<f32>,
}

struct WorldData {
    lights: array<LightData, 4>,
    ambient_color: vec4<f32>,
    use_specular: u32,
}

struct ObjectData {
    model: mat4x4<f32>,
    // Inverse-transpose of `model`, so non-uniform scale keeps normals perpendicular.
    normal_matrix: mat4x4<f32>,
    base_color: vec3<f32>,
    alpha: f32,
    // Already remapped to Disney roughness.
    roughness: f32,
    metallic: f32,
    // Negative scale flips triangle winding, and with it `front_facing`.
    mirrored: u32,
    // Material Preview: emitted light added after lighting. Black in Solid mode.
    emission: vec3<f32>,
}

@group(0) @binding(0) var<uniform> view_data: ViewData;
@group(0) @binding(1) var<uniform> world_data: WorldData;
@group(1) @binding(0) var<uniform> object_data: ObjectData;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) view_position: vec3<f32>,
    @location(1) view_normal: vec3<f32>,
}

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let world_position = object_data.model * vec4(vertex.position, 1.0);
    let world_normal = (object_data.normal_matrix * vec4(vertex.normal, 0.0)).xyz;
    var out: VertexOut;
    out.clip_position = view_data.view_projection * world_position;
    out.view_position = (view_data.view * world_position).xyz;
    out.view_normal = (view_data.view * vec4(world_normal, 0.0)).xyz;
    return out;
}

// [Drobot2014a] Low Level Optimizations for GCN; kept so results match Blender bit for bit.
fn fast_rcp(v: vec4<f32>) -> vec4<f32> {
    return bitcast<vec4<f32>>(vec4<i32>(0x7eef370b) - bitcast<vec4<i32>>(v));
}

fn brdf_approx(spec_color: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let fresnel = exp2(-8.35 * n_dot_v) * (1.0 - roughness);
    return mix(spec_color, vec3(1.0), fresnel);
}

// Normalized Blinn; pi is already divided out of the light power.
fn blinn_specular(shininess: vec4<f32>, spec_angle: vec4<f32>, n_dot_l: vec4<f32>) -> vec4<f32> {
    let normalization_factor = shininess * 0.125 + 1.0;
    return pow(spec_angle, shininess) * n_dot_l * normalization_factor;
}

// `n_dot_l` must be unclamped; `w` is in [0, 1].
fn wrapped_lighting(n_dot_l: vec4<f32>, w: vec4<f32>) -> vec4<f32> {
    let w_1 = w + 1.0;
    return clamp((n_dot_l + w) * fast_rcp(w_1 * w_1), vec4(0.0), vec4(1.0));
}

fn light_wraps() -> vec4<f32> {
    return vec4(
        world_data.lights[0].diffuse_color_wrap.a,
        world_data.lights[1].diffuse_color_wrap.a,
        world_data.lights[2].diffuse_color_wrap.a,
        world_data.lights[3].diffuse_color_wrap.a,
    );
}

// Specular light reaching the surface from the four lights plus the simulated environment.
fn specular_lighting(roughness: f32, n: vec3<f32>, i: vec3<f32>) -> vec3<f32> {
    let wrap = light_wraps();
    let r = -reflect(i, n);
    var spec_angle: vec4<f32>;
    var spec_n_dot_l: vec4<f32>;
    var wrapped_n_dot_l: vec4<f32>;
    for (var k = 0; k < 4; k++) {
        let l = world_data.lights[k].direction.xyz;
        wrapped_n_dot_l[k] = dot(l, r);
        spec_angle[k] = saturate(dot(normalize(l + i), n));
        spec_n_dot_l[k] = saturate(dot(l, n));
    }
    // Smooth lights get less gloss, simulating a bigger light.
    let gloss = vec4(1.0 - roughness) * (1.0 - wrap);
    let shininess = exp2(10.0 * gloss + 1.0);
    let spec_env = wrapped_lighting(wrapped_n_dot_l, mix(wrap, vec4(1.0), roughness));
    let spec_light = mix(blinn_specular(shininess, spec_angle, spec_n_dot_l), spec_env, wrap * wrap);

    var light = world_data.ambient_color.rgb;
    for (var k = 0; k < 4; k++) {
        light += spec_light[k] * world_data.lights[k].specular_color.rgb;
    }
    return light;
}

fn world_lighting(base_color: vec3<f32>, roughness: f32, metallic: f32, n: vec3<f32>, i: vec3<f32>) -> vec3<f32> {
    var diffuse_color = base_color;
    var specular_color = vec3(0.0);
    var specular_light = world_data.ambient_color.rgb;
    if (world_data.use_specular != 0u) {
        diffuse_color = mix(base_color, vec3(0.0), metallic);
        specular_color = brdf_approx(mix(vec3(0.05), base_color, metallic), roughness, saturate(dot(n, i)));
        specular_light = specular_lighting(roughness, n, i);
    }
    specular_light *= specular_color;

    var diff_n_dot_l: vec4<f32>;
    for (var k = 0; k < 4; k++) {
        diff_n_dot_l[k] = dot(world_data.lights[k].direction.xyz, n);
    }
    let diff_light = wrapped_lighting(diff_n_dot_l, light_wraps());
    var diffuse_light = world_data.ambient_color.rgb;
    for (var k = 0; k < 4; k++) {
        diffuse_light += diff_light[k] * world_data.lights[k].diffuse_color_wrap.rgb;
    }
    // Monochrome specular energy, so coloured specular doesn't tint the diffuse.
    let spec_energy = dot(specular_color, vec3(0.33333));
    diffuse_light *= diffuse_color * (1.0 - spec_energy);
    return diffuse_light + specular_light;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let facing = front_facing != (object_data.mirrored != 0u);
    // Back faces are lit from their own side, as workbench's `normal_encode` does.
    let n = normalize(select(-in.view_normal, in.view_normal, facing));
    let i = normalize(-in.view_position);
    let color = world_lighting(object_data.base_color, object_data.roughness, object_data.metallic, n, i);
    // Opaque slots have alpha 1; the blended pipeline uses it for "over" blending.
    return vec4(color + object_data.emission, object_data.alpha);
}
