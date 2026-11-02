//! GPU-free backend: writes a textured cube GLB (plus optional OBJ/PLY) so the whole
//! queue / UI / CLI / engine-import flow can be exercised without TRELLIS.2 installed.
use anyhow::Result;
use serde_json::json;
use std::path::Path;

const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
    ([0., 0., 1.], [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]]),
    ([0., 0., -1.], [[1., -1., -1.], [-1., -1., -1.], [-1., 1., -1.], [1., 1., -1.]]),
    ([1., 0., 0.], [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]]),
    ([-1., 0., 0.], [[-1., -1., -1.], [-1., -1., 1.], [-1., 1., 1.], [-1., 1., -1.]]),
    ([0., 1., 0.], [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]]),
    ([0., -1., 0.], [[-1., -1., -1.], [1., -1., -1.], [1., -1., 1.], [-1., -1., 1.]]),
];
const UVS: [[f32; 2]; 4] = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];

struct Geo {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u16>,
}

fn cube(aspect: f32) -> Geo {
    let mut g = Geo { pos: vec![], nrm: vec![], uv: vec![], idx: vec![] };
    for (n, corners) in FACES.iter() {
        let base = g.pos.len() as u16;
        for (i, c) in corners.iter().enumerate() {
            g.pos.push([c[0] * 0.5 * aspect, c[1] * 0.5, c[2] * 0.5]);
            g.nrm.push(*n);
            g.uv.push(UVS[i]);
        }
        g.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    g
}

fn pad4(v: &mut Vec<u8>, byte: u8) {
    while v.len() % 4 != 0 {
        v.push(byte);
    }
}

fn floats(v: &mut Vec<u8>, data: impl Iterator<Item = f32>) {
    for f in data {
        v.extend_from_slice(&f.to_le_bytes());
    }
}

pub fn write_glb(image: &Path, dest: &Path) -> Result<()> {
    let (w, h) = imagesize::size(image).map(|s| (s.width as f32, s.height as f32)).unwrap_or((1., 1.));
    let g = cube((w / h).clamp(0.25, 4.0));
    let ext = image.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        _ => None, // webp would need EXT_texture_webp; mock simply goes untextured
    };

    let mut bin: Vec<u8> = vec![];
    let mut views = vec![];
    let mut push_view = |bin: &mut Vec<u8>, start: usize, target: Option<u32>| {
        let mut v = json!({"buffer":0,"byteOffset":start,"byteLength":bin.len()-start});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        views.push(v);
        pad4(bin, 0);
    };

    let s = bin.len();
    floats(&mut bin, g.pos.iter().flatten().copied());
    push_view(&mut bin, s, Some(34962));
    let s = bin.len();
    floats(&mut bin, g.nrm.iter().flatten().copied());
    push_view(&mut bin, s, Some(34962));
    let s = bin.len();
    floats(&mut bin, g.uv.iter().flatten().copied());
    push_view(&mut bin, s, Some(34962));
    let s = bin.len();
    for i in &g.idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    push_view(&mut bin, s, Some(34963));
    if mime.is_some() {
        let s = bin.len();
        bin.extend(std::fs::read(image)?);
        push_view(&mut bin, s, None);
    }

    let mut mn = [f32::MAX; 3];
    let mut mx = [f32::MIN; 3];
    for p in &g.pos {
        for k in 0..3 {
            mn[k] = mn[k].min(p[k]);
            mx[k] = mx[k].max(p[k]);
        }
    }
    let mut material = json!({"pbrMetallicRoughness":{"metallicFactor":0.0,"roughnessFactor":0.8}});
    let mut doc = json!({
        "asset":{"version":"2.0","generator":"TRELLIS Studio (mock backend)"},
        "scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"name":"mock_asset"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2},"indices":3,"material":0}]}],
        "accessors":[
            {"bufferView":0,"componentType":5126,"count":g.pos.len(),"type":"VEC3","min":mn,"max":mx},
            {"bufferView":1,"componentType":5126,"count":g.nrm.len(),"type":"VEC3"},
            {"bufferView":2,"componentType":5126,"count":g.uv.len(),"type":"VEC2"},
            {"bufferView":3,"componentType":5123,"count":g.idx.len(),"type":"SCALAR"}
        ],
    });
    if let Some(m) = mime {
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index":0});
        doc["images"] = json!([{"bufferView":4,"mimeType":m}]);
        doc["textures"] = json!([{"source":0,"sampler":0}]);
        doc["samplers"] = json!([{"magFilter":9729,"minFilter":9987,"wrapS":33071,"wrapT":33071}]);
    }
    doc["materials"] = json!([material]);
    doc["bufferViews"] = json!(views);
    doc["buffers"] = json!([{"byteLength":bin.len()}]);

    let mut js = serde_json::to_vec(&doc)?;
    pad4(&mut js, b' ');
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    std::fs::write(dest, out)?;
    Ok(())
}

pub fn write_obj(dest: &Path) -> Result<()> {
    let g = cube(1.0);
    let mut s = String::from("# TRELLIS Studio mock cube\no mock_asset\n");
    for p in &g.pos {
        s += &format!("v {} {} {}\n", p[0], p[1], p[2]);
    }
    for t in &g.uv {
        s += &format!("vt {} {}\n", t[0], t[1]);
    }
    for n in &g.nrm {
        s += &format!("vn {} {} {}\n", n[0], n[1], n[2]);
    }
    for t in g.idx.chunks(3) {
        let f: Vec<String> = t.iter().map(|i| format!("{0}/{0}/{0}", i + 1)).collect();
        s += &format!("f {}\n", f.join(" "));
    }
    std::fs::write(dest, s)?;
    Ok(())
}

pub fn write_ply(dest: &Path) -> Result<()> {
    let g = cube(1.0);
    let mut s = format!(
        "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nelement face {}\nproperty list uchar int vertex_indices\nend_header\n",
        g.pos.len(),
        g.idx.len() / 3
    );
    for p in &g.pos {
        s += &format!("{} {} {}\n", p[0], p[1], p[2]);
    }
    for t in g.idx.chunks(3) {
        s += &format!("3 {} {} {}\n", t[0], t[1], t[2]);
    }
    std::fs::write(dest, s)?;
    Ok(())
}
