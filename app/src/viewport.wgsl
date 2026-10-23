// Calibro viewport shaders. Lighting is computed in linear space; the output
// is gamma-encoded by hand (the target is a plain Rgba8Unorm texture that egui
// samples as ordinary gamma-space image data).

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // xyz: key light direction (towards the light), w: unused
    light: vec4<f32>,
    // section plane: xyz normal, w offset (fragments with dot(p,n) > w are cut)
    clip: vec4<f32>,
    // x: section enabled, y: viewport height px, z/w: unused
    flags: vec4<f32>,
};

struct Object {
    // base colour (sRGB, alpha = opacity)
    color: vec4<f32>,
    // highlight colour (rgb) and body-selected amount (a)
    highlight: vec4<f32>,
    // x: selected face id (-1 none), y: hovered face id (-1 none)
    ids: vec4<i32>,
    // x: metallic, y: roughness, z: x-ray opacity override (0 = use colour alpha)
    material: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> o: Object;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

fn to_gamma(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), c.a);
}

fn sky(dir: vec3<f32>) -> vec3<f32> {
    // simple studio environment: bright top, warm horizon band, darker floor
    let t = clamp(dir.z * 0.5 + 0.5, 0.0, 1.0);
    let floor_c = vec3<f32>(0.18, 0.18, 0.2);
    let horizon = vec3<f32>(0.85, 0.85, 0.86);
    let top = vec3<f32>(0.62, 0.68, 0.78);
    let band = exp(-abs(dir.z) * 7.0) * 0.6;
    var c = mix(floor_c, top, smoothstep(0.35, 0.9, t));
    c = mix(c, horizon, band);
    return c;
}

fn clipped(p: vec3<f32>) -> bool {
    return g.flags.x > 0.5 && dot(p, g.clip.xyz) > g.clip.w;
}

// ---------------------------------------------------------------- meshes

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) face: u32,
};

struct MeshOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) @interpolate(flat) face: u32,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var out: MeshOut;
    out.clip_pos = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.nor = v.nor;
    out.face = v.face;
    return out;
}

fn shade(in: MeshOut, front: bool) -> vec4<f32> {
    var n = normalize(in.nor);
    if (!front) {
        n = -n;
    }
    let v = normalize(g.eye.xyz - in.world);
    let l = normalize(g.light.xyz);
    let h = normalize(l + v);
    let base = to_linear(o.color.rgb);
    let metallic = clamp(o.material.x, 0.0, 1.0);
    let rough = clamp(o.material.y, 0.04, 1.0);

    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 0.0);
    let shin = mix(400.0, 6.0, rough);
    let spec = pow(max(dot(n, h), 0.0), shin) * (shin + 8.0) / 80.0;
    let f0 = mix(vec3<f32>(0.04), base, metallic);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - ndv, 5.0) * (1.0 - rough);

    // fill light from the opposite side, hemisphere ambient
    let fill = max(dot(n, normalize(vec3<f32>(-l.x, -l.y, 0.4))), 0.0) * 0.2;
    let ambient = mix(vec3<f32>(0.36, 0.36, 0.39), vec3<f32>(0.66, 0.68, 0.72), n.z * 0.5 + 0.5);
    let env = sky(reflect(-v, n));

    let diffuse = base * (1.0 - metallic) * (ambient * 0.95 + vec3<f32>(ndl * 0.65 + fill));
    let specular = fresnel * (spec * ndl + env * mix(0.25, 1.0, metallic) * (1.0 - rough * 0.7));
    var c = diffuse + specular;

    // selection / hover highlights
    let hl = to_linear(o.highlight.rgb);
    if (i32(in.face) == o.ids.x) {
        c = mix(c, hl, 0.55);
    } else if (i32(in.face) == o.ids.y) {
        c = mix(c, hl, 0.16);
    }
    c = mix(c, hl, o.highlight.a);

    var alpha = o.color.a;
    if (o.material.z > 0.0) {
        alpha = o.material.z;
    }
    return vec4<f32>(c, alpha);
}

@fragment
fn fs_mesh(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    return to_gamma(shade(in, front));
}

// ---------------------------------------------------------------- lines & fills

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var out: LineOut;
    out.clip_pos = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.color = v.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    // line colours are already sRGB values
    return in.color;
}

// ---------------------------------------------------------------- background

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var out: BgOut;
    out.pos = vec4<f32>(p[i], 0.0, 1.0);
    out.uv = p[i] * 0.5 + vec2<f32>(0.5);
    return out;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    let top = vec3<f32>(0.085, 0.135, 0.205);
    let bottom = vec3<f32>(0.13, 0.20, 0.29);
    return vec4<f32>(mix(bottom, top, in.uv.y), 1.0);
}
