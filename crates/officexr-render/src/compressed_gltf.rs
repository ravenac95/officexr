//! A GLB -> GLB transform that expands `EXT_meshopt_compression` buffer views
//! and turns quantized vertex attributes into floats, so `bevy_gltf` (which
//! reads neither extension) can load files compressed with gltf-transform.

use std::borrow::Cow;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::gltf::{Gltf, GltfError, GltfLoader, GltfLoaderSettings};
use bevy::reflect::TypePath;
use meshopt_rs::index::buffer::decode_index_buffer;
use meshopt_rs::vertex::buffer::decode_vertex_buffer;
use meshopt_rs::vertex::filter;
use serde_json::{Value, json};

const MESHOPT: &str = "EXT_meshopt_compression";
const QUANTIZATION: &str = "KHR_mesh_quantization";
const GLB_MAGIC: &[u8; 4] = b"glTF";
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;
const FLOAT: u64 = 5126;

type Result<T> = std::result::Result<T, String>;

/// Returns `glb` unchanged unless it uses meshopt or mesh quantization.
pub fn decode_compressed_glb(glb: &[u8]) -> Result<Cow<'_, [u8]>> {
    if glb.get(..4) != Some(GLB_MAGIC) {
        return Ok(Cow::Borrowed(glb));
    }
    let (json_bytes, bin) = chunks(glb)?;
    let mut doc: Value = serde_json::from_slice(json_bytes).map_err(|e| e.to_string())?;
    let uses = |name: &str| {
        doc["extensionsUsed"]
            .as_array()
            .is_some_and(|used| used.iter().any(|u| u == name))
    };
    if !uses(MESHOPT) && !uses(QUANTIZATION) {
        return Ok(Cow::Borrowed(glb));
    }

    let mut out = Vec::new();
    rewrite_buffer_views(&mut doc, bin, &mut out)?;
    dequantize_attributes(&mut doc, &mut out)?;
    doc["buffers"] = json!([{ "byteLength": out.len() }]);
    let root = doc.as_object_mut().ok_or("glTF root is not an object")?;
    root.remove("extensionsUsed");
    root.remove("extensionsRequired");
    Ok(Cow::Owned(write_glb(&doc, &out)?))
}

fn chunks(glb: &[u8]) -> Result<(&[u8], &[u8])> {
    let (mut json, mut bin) = (None, &[][..]);
    let mut at = 12;
    while at + 8 <= glb.len() {
        let len = u32_at(glb, at)? as usize;
        let data = glb
            .get(at + 8..at + 8 + len)
            .ok_or("GLB chunk overruns the file")?;
        match u32_at(glb, at + 4)? {
            JSON_CHUNK => json = Some(data),
            BIN_CHUNK => bin = data,
            _ => {}
        }
        at += 8 + len.next_multiple_of(4);
    }
    Ok((json.ok_or("GLB has no JSON chunk")?, bin))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let word = bytes.get(at..at + 4).ok_or("truncated GLB")?;
    Ok(u32::from_le_bytes(word.try_into().unwrap()))
}

fn write_glb(doc: &Value, bin: &[u8]) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec(doc).map_err(|e| e.to_string())?;
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bin = bin.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(GLB_MAGIC);
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    for (kind, data) in [(JSON_CHUNK, &json), (BIN_CHUNK, &bin)] {
        glb.extend_from_slice(&(data.len() as u32).to_le_bytes());
        glb.extend_from_slice(&kind.to_le_bytes());
        glb.extend_from_slice(data);
    }
    Ok(glb)
}

/// Appends `data` to `out`, 4-byte aligned, and returns its offset.
fn append(out: &mut Vec<u8>, data: &[u8]) -> usize {
    out.resize(out.len().next_multiple_of(4), 0);
    let offset = out.len();
    out.extend_from_slice(data);
    offset
}

fn field(v: &Value, key: &str) -> usize {
    v[key].as_u64().unwrap_or(0) as usize
}

/// Moves every buffer view into `out` (the GLB's only buffer), expanding the
/// meshopt-compressed ones.
fn rewrite_buffer_views(doc: &mut Value, bin: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let views = doc["bufferViews"].as_array_mut().into_iter().flatten();
    for view in views {
        let data = match view["extensions"].get(MESHOPT) {
            Some(ext) => {
                let start = field(ext, "byteOffset");
                let src = bin
                    .get(start..start + field(ext, "byteLength"))
                    .ok_or("meshopt view overruns the buffer")?;
                decode_meshopt(ext, src)?
            }
            None => {
                if field(view, "buffer") != 0 {
                    return Err("only the GLB's own buffer is supported".into());
                }
                let start = field(view, "byteOffset");
                bin.get(start..start + field(view, "byteLength"))
                    .ok_or("buffer view overruns the buffer")?
                    .to_vec()
            }
        };
        let offset = append(out, &data);
        view["buffer"] = json!(0);
        view["byteOffset"] = json!(offset);
        view["byteLength"] = json!(data.len());
        if let Some(ext) = view.as_object_mut() {
            ext.remove("extensions");
        }
    }
    Ok(())
}

fn decode_meshopt(ext: &Value, src: &[u8]) -> Result<Vec<u8>> {
    let (count, stride) = (field(ext, "count"), field(ext, "byteStride"));
    let mode = ext["mode"].as_str().unwrap_or_default();
    // ponytail: INDICES (index sequence) mode is rejected; gltf-transform emits it only for non-triangle primitives.
    let mut data = match (mode, stride) {
        ("ATTRIBUTES", _) => decode_attributes(src, count, stride)?,
        ("TRIANGLES", 2 | 4) => {
            let mut idx = vec![0u32; count];
            decode_index_buffer(&mut idx, src)
                .map_err(|e| format!("meshopt index decode failed: {e:?}"))?;
            if stride == 2 {
                idx.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect()
            } else {
                idx.iter().flat_map(|i| i.to_le_bytes()).collect()
            }
        }
        _ => return Err(format!("unsupported meshopt mode {mode} stride {stride}")),
    };
    apply_filter(ext["filter"].as_str().unwrap_or("NONE"), &mut data, stride)?;
    Ok(data)
}

fn decode_attributes(src: &[u8], count: usize, stride: usize) -> Result<Vec<u8>> {
    // ponytail: vertex strides above 16 bytes (interleaved attributes) are rejected.
    macro_rules! decode {
        ($($n:literal),*) => {
            match stride {
                $($n => {
                    let mut v = vec![[0u8; $n]; count];
                    decode_vertex_buffer(&mut v, src)
                        .map_err(|e| format!("meshopt vertex decode failed: {e:?}"))?;
                    v.as_flattened().to_vec()
                })*
                _ => return Err(format!("unsupported meshopt vertex stride {stride}")),
            }
        };
    }
    Ok(decode!(4, 8, 12, 16))
}

fn apply_filter(name: &str, data: &mut Vec<u8>, stride: usize) -> Result<()> {
    match (name, stride) {
        ("NONE", _) => {}
        ("OCTAHEDRAL", 4) => {
            let mut v = data.as_chunks::<4>().0.to_vec();
            filter::decode_filter_oct_8(&mut v);
            *data = v.as_flattened().to_vec();
        }
        ("OCTAHEDRAL" | "QUATERNION", 8) => {
            let mut v: Vec<[u16; 4]> = data
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| std::array::from_fn(|i| u16::from_le_bytes([c[2 * i], c[2 * i + 1]])))
                .collect();
            if name == "OCTAHEDRAL" {
                filter::decode_filter_oct_16(&mut v);
            } else {
                filter::decode_filter_quat(&mut v);
            }
            *data = v.iter().flatten().flat_map(|w| w.to_le_bytes()).collect();
        }
        ("EXPONENTIAL", _) => {
            let mut v: Vec<u32> = data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            filter::decode_filter_exp(&mut v);
            *data = v.iter().flat_map(|w| w.to_le_bytes()).collect();
        }
        _ => return Err(format!("unsupported meshopt filter {name} stride {stride}")),
    }
    Ok(())
}

/// Rewrites every non-float mesh attribute accessor (joint indices excepted)
/// as a tightly packed float buffer view.
fn dequantize_attributes(doc: &mut Value, out: &mut Vec<u8>) -> Result<()> {
    let mut accessors = Vec::new();
    for prim in doc["meshes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["primitives"].as_array().into_iter().flatten())
    {
        let targets = prim["targets"].as_array().into_iter().flatten();
        for set in std::iter::once(&prim["attributes"]).chain(targets) {
            for (name, idx) in set.as_object().into_iter().flatten() {
                if !name.starts_with("JOINTS_") {
                    accessors.extend(idx.as_u64());
                }
            }
        }
    }
    accessors.sort_unstable();
    accessors.dedup();

    for idx in accessors {
        let acc = &doc["accessors"][idx as usize];
        if acc["componentType"].as_u64() == Some(FLOAT) {
            continue;
        }
        let floats = read_as_floats(doc, acc, out)?;
        let comps = components(acc["type"].as_str().unwrap_or_default())?;
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let view = doc["bufferViews"].as_array().map_or(0, Vec::len);
        let offset = append(out, &bytes);
        doc["bufferViews"]
            .as_array_mut()
            .ok_or("missing bufferViews")?
            .push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": 34962 }));
        let acc = &mut doc["accessors"][idx as usize];
        acc["bufferView"] = json!(view);
        acc["byteOffset"] = json!(0);
        acc["componentType"] = json!(FLOAT);
        acc.as_object_mut()
            .ok_or("accessor is not an object")?
            .remove("normalized");
        let (min, max) = bounds(&floats, comps);
        acc["min"] = json!(min);
        acc["max"] = json!(max);
    }
    Ok(())
}

fn components(ty: &str) -> Result<usize> {
    Ok(match ty {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => return Err(format!("unsupported attribute accessor type {ty}")),
    })
}

fn bounds(floats: &[f32], comps: usize) -> (Vec<f32>, Vec<f32>) {
    let mut min = vec![f32::INFINITY; comps];
    let mut max = vec![f32::NEG_INFINITY; comps];
    for v in floats.chunks_exact(comps) {
        for (i, x) in v.iter().enumerate() {
            min[i] = min[i].min(*x);
            max[i] = max[i].max(*x);
        }
    }
    (min, max)
}

fn read_as_floats(doc: &Value, acc: &Value, bin: &[u8]) -> Result<Vec<f32>> {
    if acc.get("sparse").is_some() {
        return Err("sparse quantized accessors are not supported".into());
    }
    let view = acc["bufferView"]
        .as_u64()
        .and_then(|i| doc["bufferViews"].get(i as usize))
        .ok_or("accessor has no buffer view")?;
    let comps = components(acc["type"].as_str().unwrap_or_default())?;
    let (size, signed) = match acc["componentType"].as_u64() {
        Some(5120) => (1, true),
        Some(5121) => (1, false),
        Some(5122) => (2, true),
        Some(5123) => (2, false),
        other => return Err(format!("unsupported component type {other:?}")),
    };
    let normalized = acc["normalized"].as_bool().unwrap_or(false);
    let stride = match field(view, "byteStride") {
        0 => comps * size,
        stride => stride,
    };
    let base = field(view, "byteOffset") + field(acc, "byteOffset");
    let mut floats = Vec::with_capacity(field(acc, "count") * comps);
    for i in 0..field(acc, "count") {
        for c in 0..comps {
            let at = base + i * stride + c * size;
            let raw = bin
                .get(at..at + size)
                .ok_or("accessor overruns its buffer view")?;
            let (value, max) = match (size, signed) {
                (1, true) => (raw[0] as i8 as f32, 127.0),
                (1, false) => (raw[0] as f32, 255.0),
                (2, true) => (i16::from_le_bytes([raw[0], raw[1]]) as f32, 32767.0),
                _ => (u16::from_le_bytes([raw[0], raw[1]]) as f32, 65535.0),
            };
            floats.push(if normalized {
                (value / max).max(-1.0)
            } else {
                value
            });
        }
    }
    Ok(floats)
}

/// Loads `.gltf`/`.glb` like `bevy_gltf`, after expanding meshopt and
/// quantization. Registered after `GltfPlugin`, so it takes over both extensions.
#[derive(TypePath)]
pub struct CompressedGltfLoader(pub GltfLoader);

impl AssetLoader for CompressedGltfLoader {
    type Asset = Gltf;
    type Settings = GltfLoaderSettings;
    type Error = GltfError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &GltfLoaderSettings,
        load_context: &mut LoadContext<'_>,
    ) -> std::result::Result<Gltf, GltfError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let bytes = decode_compressed_glb(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        GltfLoader::load_gltf(&self.0, &bytes, load_context, settings).await
    }

    fn extensions(&self) -> &[&str] {
        &["gltf", "glb"]
    }
}

#[cfg(test)]
mod tests {
    use bevy::gltf::gltf::Gltf;

    use super::*;

    const COMPRESSED: &[u8] = include_bytes!("../tests/fixtures/compressed.glb");
    /// The same mesh, decoded by gltf-transform.
    const REFERENCE: &[u8] = include_bytes!("../tests/fixtures/reference.glb");

    /// Every primitive's `(indices, positions, normals, uvs)`.
    fn meshes(glb: &[u8]) -> Vec<(Vec<u32>, Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>)> {
        let gltf = Gltf::from_slice(glb).expect("valid glTF");
        let blob = gltf.blob.as_deref();
        gltf.meshes()
            .flat_map(|m| m.primitives())
            .map(|p| {
                let r = p.reader(|_| blob);
                (
                    r.read_indices().unwrap().into_u32().collect(),
                    r.read_positions().unwrap().collect(),
                    r.read_normals().unwrap().collect(),
                    r.read_tex_coords(0).unwrap().into_f32().collect(),
                )
            })
            .collect()
    }

    fn assert_close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-4, "{x} != {y}");
        }
    }

    #[test]
    fn decodes_meshopt_and_quantized_meshes_like_gltf_transform() {
        let decoded = decode_compressed_glb(COMPRESSED).unwrap();
        let (got, want) = (meshes(&decoded), meshes(REFERENCE));
        assert_eq!(got.len(), 1);
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(g.0, w.0);
            assert_close(g.1.as_flattened(), w.1.as_flattened());
            assert_close(g.2.as_flattened(), w.2.as_flattened());
            assert_close(g.3.as_flattened(), w.3.as_flattened());
        }
    }

    #[test]
    fn leaves_plain_glbs_alone() {
        assert!(matches!(
            decode_compressed_glb(REFERENCE).unwrap(),
            Cow::Borrowed(_)
        ));
    }
}
