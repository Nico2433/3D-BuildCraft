//! WGSL for every viewport pass: meshes, floor, shadows, slab and the selection outline.

pub(super) const SHADER: &str = r#"
struct Globals { view_proj: mat4x4<f32>, eye: vec4<f32>, light: vec4<f32>, flags: vec4<f32>, flags2: vec4<f32>, viewport: vec4<f32>, clip: vec4<f32>, decal: mat4x4<f32>, decal_n: vec4<f32>, decal_p: vec4<f32> };
struct Obj { model: mat4x4<f32>, normal: mat4x4<f32>, color: vec4<f32>, extra: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> o: Obj;
@group(2) @binding(0) var tex: texture_2d_array<f32>;
@group(2) @binding(1) var samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) wpos: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, @location(3) uv: vec2<f32>, @location(4) @interpolate(flat) tx: u32 };

@vertex fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, @location(3) uv: vec2<f32>, @location(4) tx: u32) -> VOut {
    let w = o.model * vec4<f32>(p, 1.0);
    var out: VOut;
    out.pos = g.view_proj * w;
    out.wpos = w.xyz;
    out.n = (o.normal * vec4<f32>(n, 0.0)).xyz;
    out.c = c;
    out.uv = uv;
    out.tx = tx;
    return out;
}

@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
    if (dot(in.wpos, g.clip.xyz) > g.clip.w) { discard; }
    // Both pictures are sampled here, in uniform control flow, so they get mip levels.
    let layer = i32(in.tx & 0xFFFFu) - 1;
    let sticker_face = (in.tx & 0x10000u) != 0u;
    var tuv = in.uv;
    if (sticker_face) { tuv = clamp(tuv, vec2<f32>(0.002), vec2<f32>(0.998)); }
    let face_tex = textureSample(tex, samp, tuv, max(layer, 0));
    let dq = (g.decal * vec4<f32>(in.wpos, 1.0)).xyz;
    let live_tex = textureSample(tex, samp, clamp(dq.xy, vec2<f32>(0.002), vec2<f32>(0.998)), max(i32(g.decal_p.x) - 1, 0));
    let flat_n = normalize(cross(dpdx(in.wpos), dpdy(in.wpos)));
    // Degenerate faces have no normal; use the screen-space one.
    var n = flat_n;
    if (length(in.n) > 1e-6) { n = normalize(in.n); }
    if (g.flags.y < 0.5) { n = flat_n; }
    let v = normalize(g.eye.xyz - in.wpos);
    if (dot(n, v) < 0.0) { n = -n; }
    var base = o.color.rgb;
    // See-through items (the scene behind Custom or an import) keep their one colour.
    if (in.c.a > 0.001 && o.color.a > 0.99) { base = in.c.rgb; }
    if (o.color.a > 0.99) {
        var inside = true;
        if (sticker_face) { inside = all(in.uv >= vec2<f32>(0.0)) && all(in.uv <= vec2<f32>(1.0)); }
        if (layer >= 0 && inside) { base = mix(base, face_tex.rgb, face_tex.a); }
        // The sticker being placed, shown on the object under the pointer.
        let facing = dot(normalize(in.n), g.decal_n.xyz) > 0.25;
        let within = all(dq.xy >= vec2<f32>(0.0)) && all(dq.xy <= vec2<f32>(1.0)) && abs(dq.z) < g.decal_n.w;
        if (g.decal_p.x > 0.5 && o.extra.w > 0.5 && o.extra.y < 0.5 && facing && within) {
            base = mix(base, live_tex.rgb, live_tex.a);
        }
    }
    if (g.flags.w < 0.5) { base = vec3<f32>(0.82, 0.82, 0.82); }
    var c = base;
    if (g.flags.x > 0.5) {
        let l = normalize(g.light.xyz);
        let diff = max(dot(n, l), 0.0);
        let fill = max(dot(n, normalize(vec3<f32>(0.5, 0.6, 0.4))), 0.0) * 0.2;
        let h = normalize(l + v);
        var spec = pow(max(dot(n, h), 0.0), 48.0) * 0.12;
        c = base * (0.40 + 0.42 * diff + fill);
        if (o.extra.z > 0.5) {
            // Realistic: a tight strong highlight and a pale sheen at grazing angles.
            spec = pow(max(dot(n, h), 0.0), 60.0) * 1.1;
            let sheen = pow(1.0 - max(dot(n, v), 0.0), 2.0) * 0.5;
            c = mix(c, vec3<f32>(0.85, 0.92, 1.0), sheen);
        }
        c = c + vec3<f32>(spec);
    }
    var a = o.color.a;
    if (g.flags.z > 0.5) { a = 0.35; }
    if (o.extra.y > 0.5) {
        // Reflection under the floor, fading with depth. Parts of the object below the
        // floor would mirror above it; they are not drawn.
        if (in.wpos.z > 0.0) { discard; }
        a = a * 0.5 * clamp(1.0 + in.wpos.z / 45.0, 0.0, 1.0);
    }
    return vec4<f32>(c, a);
}

// A depth-free mask covers the projected union, including concave interiors.
struct MaskOut { @builtin(position) pos: vec4<f32>, @location(0) wpos: vec3<f32> };
@vertex fn vs_mask(@location(0) p: vec3<f32>) -> MaskOut {
    let w = o.model * vec4<f32>(p, 1.0);
    return MaskOut(g.view_proj * w, w.xyz);
}
@fragment fn fs_mask(in: MaskOut) -> @location(0) vec4<f32> {
    if (dot(in.wpos, g.clip.xyz) > g.clip.w) { discard; }
    return vec4<f32>(1.0);
}

@group(0) @binding(0) var mask: texture_2d<f32>;
struct OutlineOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_screen(@builtin(vertex_index) i: u32) -> OutlineOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: OutlineOut;
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}
fn coverage(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(mask));
    if (any(p < vec2<i32>(0)) || any(p >= size)) { return 0.0; }
    return textureLoad(mask, p, 0).r;
}
@fragment fn fs_dilate(in: OutlineOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    if (coverage(p) > 0.5) { return vec4<f32>(0.0); }
    var edge = 0.0;
    // A circular four-pixel dilation has a constant width at every crease.
    for (var y = -4; y <= 4; y++) {
        for (var x = -4; x <= 4; x++) {
            if (x*x + y*y <= 16) { edge = max(edge, coverage(p + vec2<i32>(x, y))); }
        }
    }
    return vec4<f32>(edge);
}
@fragment fn fs_outline(in: OutlineOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.uv * vec2<f32>(textureDimensions(mask)));
    return vec4<f32>(0.62, 0.78, 0.92, coverage(p));
}

struct SOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_shadow(@builtin(vertex_index) i: u32) -> SOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    var out: SOut;
    out.pos = g.view_proj * (o.model * vec4<f32>(corners[i], 0.0, 1.0));
    out.uv = corners[i];
    return out;
}

@fragment fn fs_shadow(in: SOut) -> @location(0) vec4<f32> {
    // A soft rounded patch under the object.
    let d = length(max(abs(in.uv) - vec2<f32>(0.5), vec2<f32>(0.0))) / 0.5;
    return vec4<f32>(0.0, 0.0, 0.0, 0.2 * (1.0 - smoothstep(0.0, 1.0, d)));
}

@vertex fn vs_wire(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> @builtin(position) vec4<f32> {
    var pos = g.view_proj * (o.model * vec4<f32>(p, 1.0));
    pos.z = pos.z - 0.00002 * pos.w;
    return pos;
}

@fragment fn fs_wire() -> @location(0) vec4<f32> {
    return vec4<f32>(0.05, 0.08, 0.12, 0.85);
}

struct SlabOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_slab(@builtin(vertex_index) i: u32) -> SlabOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    var out: SlabOut;
    out.pos = g.view_proj * o.model * vec4<f32>(corners[i], 0.0, 1.0);
    out.uv = corners[i];
    return out;
}

// Translucent fill with a solid edge line a pixel or two wide.
@fragment fn fs_slab(in: SlabOut) -> @location(0) vec4<f32> {
    let edge = (1.0 - abs(in.uv)) / fwidth(in.uv);
    if (min(edge.x, edge.y) < 1.5) { return vec4<f32>(o.color.rgb, 1.0); }
    return vec4<f32>(o.color.rgb, o.color.a);
}

struct FOut { @builtin(position) pos: vec4<f32>, @location(0) w: vec2<f32> };

@vertex fn vs_floor(@builtin(vertex_index) i: u32) -> FOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let w = corners[i] * 3000.0;
    var out: FOut;
    out.pos = g.view_proj * vec4<f32>(w, -0.02, 1.0);
    out.w = w;
    return out;
}

@fragment fn fs_floor(in: FOut) -> @location(0) vec4<f32> {
    let cell = floor(in.w / 10.0);
    let odd = (i32(cell.x) + i32(cell.y)) & 1;
    var c = select(0.375, 0.445, odd == 1);
    // Light and dark tones from the grid properties.
    if (g.flags2.z > 1.5) { c = select(0.30, 0.34, odd == 1); }
    else if (g.flags2.z > 0.5) { c = select(0.56, 0.62, odd == 1); }
    let d = length(in.w - g.eye.xy);
    let fade = 1.0 - smoothstep(400.0, 2600.0, d);
    // Slightly see-through so reflections show under it.
    let opacity = select(1.0, 0.88, g.flags2.y > 0.5);
    return vec4<f32>(vec3<f32>(c), fade * opacity * g.flags2.x);
}
"#;
