// A retained page slab and a one-line status strip, placed by `View`.
// Window pixel (x, y): above `inset` is paper (the title strip the content
// runs under); from `status_top` down is the status texture; outside the
// band is paper, so a partially visible line never shows; inside it the
// page slab is read at row `y + scroll`.

struct View {
    scroll: i32,
    band_top: i32,
    band_bottom: i32,
    status_top: i32,
    inset: i32,
    paper: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var page: texture_2d<f32>;
@group(0) @binding(1) var status: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOut;
    output.position = vec4<f32>(positions[vertex], 0.0, 1.0);
    return output;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(input.position.xy);
    let paper_level = f32(view.paper) / 255.0;
    let paper = vec4<f32>(paper_level, paper_level, paper_level, 1.0);
    let y = p.y - view.inset;
    if (y < 0) {
        return paper;
    }
    if (y >= view.status_top) {
        let sy = y - view.status_top;
        let dims = vec2<i32>(textureDimensions(status, 0));
        if (p.x >= dims.x || sy >= dims.y) {
            return paper;
        }
        return textureLoad(status, vec2<i32>(p.x, sy), 0);
    }
    if (y < view.band_top || y >= view.band_bottom) {
        return paper;
    }
    let py = y + view.scroll;
    let dims = vec2<i32>(textureDimensions(page, 0));
    if (py < 0 || p.x >= dims.x || py >= dims.y) {
        return paper;
    }
    return textureLoad(page, vec2<i32>(p.x, py), 0);
}
