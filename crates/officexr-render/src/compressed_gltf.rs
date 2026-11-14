//! A GLB -> GLB transform that expands `EXT_meshopt_compression` buffer views
//! and turns quantized vertex attributes into floats, so `bevy_gltf` (which
//! reads neither extension) can load files compressed with gltf-transform.
//!
//! The input is untrusted network data: every malformed shape must come back
//! as an `Err`, never a panic, because a panic takes the wasm app down.

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
const ARRAY_BUFFER: u64 = 34962;
/// Caps allocations sized from untrusted JSON.
// ponytail: a single meshopt buffer view that decodes to more than 64 MiB is rejected.
const MAX_DECODED_VIEW_BYTES: usize = 1 << 26;

type Result<T> = std::result::Result<T, String>;

/// Returns `glb` unchanged unless it is a GLB that uses meshopt or mesh quantization.
// ponytail: a JSON `.gltf` file is never decoded, so a compressed one passes through unchanged.
pub(crate) fn decode_compressed_glb(glb: &[u8]) -> Result<Cow<'_, [u8]>> {
    if glb.get(..4) != Some(GLB_MAGIC) {
        return Ok(Cow::Borrowed(glb));
    }
    let (json_bytes, bin) = chunks(glb)?;
    let mut doc: Value = serde_json::from_slice(json_bytes).map_err(|e| e.to_string())?;
    if !doc.is_object() {
        return Err("glTF root is not an object".into());
    }
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
    let root = doc.as_object_mut().ok_or("glTF root is not an object")?;
    root.insert("buffers".into(), json!([{ "byteLength": out.len() }]));
    for key in ["extensionsUsed", "extensionsRequired"] {
        if let Some(names) = root.get_mut(key).and_then(Value::as_array_mut) {
            names.retain(|n| n != MESHOPT && n != QUANTIZATION);
            if names.is_empty() {
                root.remove(key);
            }
        }
    }
    Ok(Cow::Owned(write_glb(&doc, &out)?))
}

fn chunks(glb: &[u8]) -> Result<(&[u8], &[u8])> {
    let (mut json, mut bin) = (None, &[][..]);
    let mut at = 12;
    while at + 8 <= glb.len() {
        let len = u32_at(glb, at)? as usize;
        let data = slice(glb, at + 8, len).map_err(|_| "GLB chunk overruns the file")?;
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
    let word = slice(bytes, at, 4).map_err(|_| "truncated GLB")?;
    Ok(u32::from_le_bytes(word.try_into().unwrap()))
}

/// `data[start..start + len]`, or an error if the range overflows or leaves `data`.
fn slice(data: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    start
        .checked_add(len)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| "byte range out of bounds".to_string())
}

fn write_glb(doc: &Value, bin: &[u8]) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec(doc).map_err(|e| e.to_string())?;
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bin = bin.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);
    let too_big = || "decoded GLB exceeds 4 GiB".to_string();
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(GLB_MAGIC);
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&u32::try_from(total).map_err(|_| too_big())?.to_le_bytes());
    for (kind, data) in [(JSON_CHUNK, &json), (BIN_CHUNK, &bin)] {
        glb.extend_from_slice(
            &u32::try_from(data.len())
                .map_err(|_| too_big())?
                .to_le_bytes(),
        );
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

/// A non-negative integer field; absent counts as 0 (glTF's default for offsets).
fn field(v: &Value, key: &str) -> Result<usize> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(n) => n
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("`{key}` is not a valid size")),
    }
}

/// Moves every buffer view into `out` (the GLB's only buffer), expanding the
/// meshopt-compressed ones.
fn rewrite_buffer_views(doc: &mut Value, bin: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let views = doc["bufferViews"].as_array_mut().into_iter().flatten();
    for view in views {
        let data = match view["extensions"].get(MESHOPT) {
            Some(ext) => {
                if field(ext, "buffer")? != 0 {
                    return Err("only the GLB's own buffer is supported".into());
                }
                let src = slice(bin, field(ext, "byteOffset")?, field(ext, "byteLength")?)
                    .map_err(|_| "meshopt view overruns the buffer")?;
                decode_meshopt(ext, field(view, "byteLength")?, src)?
            }
            None => {
                if field(view, "buffer")? != 0 {
                    return Err("only the GLB's own buffer is supported".into());
                }
                slice(bin, field(view, "byteOffset")?, field(view, "byteLength")?)
                    .map_err(|_| "buffer view overruns the buffer")?
                    .to_vec()
            }
        };
        let offset = append(out, &data);
        let view = view.as_object_mut().ok_or("buffer view is not an object")?;
        view.remove("extensions");
        view.insert("buffer".into(), json!(0));
        view.insert("byteOffset".into(), json!(offset));
        view.insert("byteLength".into(), json!(data.len()));
    }
    Ok(())
}

/// Decodes one meshopt view. `view_len` is the view's own (decoded) `byteLength`.
fn decode_meshopt(ext: &Value, view_len: usize, src: &[u8]) -> Result<Vec<u8>> {
    let (count, stride) = (field(ext, "count")?, field(ext, "byteStride")?);
    let decoded = count
        .checked_mul(stride)
        .ok_or("meshopt count * byteStride overflows")?;
    if decoded != view_len {
        return Err("meshopt count * byteStride does not match the buffer view length".into());
    }
    if decoded > MAX_DECODED_VIEW_BYTES {
        return Err("meshopt view is too large".into());
    }
    let mode = ext["mode"].as_str().unwrap_or_default();
    // ponytail: INDICES (index sequence) mode is rejected; gltf-transform emits it only for non-triangle primitives.
    let mut data = match (mode, stride) {
        ("ATTRIBUTES", _) => decode_attributes(src, count, stride)?,
        ("TRIANGLES", 2 | 4) => {
            if count % 3 != 0 {
                return Err("meshopt TRIANGLES count is not a multiple of 3".into());
            }
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
        ("EXPONENTIAL", s) if s % 4 == 0 => {
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

/// The attribute semantics `bevy_gltf` reads as floats; others (`JOINTS_n`,
/// custom `_NAME`s) keep their stored type.
fn is_float_semantic(name: &str) -> bool {
    matches!(name, "POSITION" | "NORMAL" | "TANGENT")
        || ["TEXCOORD_", "COLOR_", "WEIGHTS_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Rewrites every non-float standard mesh attribute accessor as a tightly
/// packed float buffer view.
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
                if is_float_semantic(name) {
                    accessors.extend(idx.as_u64().and_then(|i| usize::try_from(i).ok()));
                }
            }
        }
    }
    accessors.sort_unstable();
    accessors.dedup();

    for idx in accessors {
        let acc = doc["accessors"]
            .get(idx)
            .ok_or("attribute accessor is missing")?;
        if acc["componentType"].as_u64() == Some(FLOAT) {
            continue;
        }
        let floats = read_as_floats(doc, acc, out)?;
        let comps = components(acc["type"].as_str().unwrap_or_default())?;
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let (min, max) = bounds(&floats, comps);
        let views = doc["bufferViews"]
            .as_array_mut()
            .ok_or("missing bufferViews")?;
        let view = views.len();
        // ponytail: the quantized source bytes stay in the BIN chunk, unreferenced.
        let offset = append(out, &bytes);
        views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": ARRAY_BUFFER }));
        let acc = doc["accessors"]
            .get_mut(idx)
            .and_then(Value::as_object_mut)
            .ok_or("accessor is not an object")?;
        acc.remove("normalized");
        acc.insert("bufferView".into(), json!(view));
        acc.insert("byteOffset".into(), json!(0));
        acc.insert("componentType".into(), json!(FLOAT));
        acc.insert("min".into(), json!(min));
        acc.insert("max".into(), json!(max));
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

/// Reads accessor `acc` out of `out` (where `doc`'s buffer views now live) as floats.
// ponytail: quantized accessors without a bufferView (all zeros) are not supported.
fn read_as_floats(doc: &Value, acc: &Value, out: &[u8]) -> Result<Vec<f32>> {
    if acc.get("sparse").is_some() {
        return Err("sparse quantized accessors are not supported".into());
    }
    let view = acc["bufferView"]
        .as_u64()
        .and_then(|i| doc["bufferViews"].get(usize::try_from(i).ok()?))
        .ok_or("accessor has no buffer view")?;
    let region = slice(out, field(view, "byteOffset")?, field(view, "byteLength")?)
        .map_err(|_| "buffer view overruns the buffer")?;
    let comps = components(acc["type"].as_str().unwrap_or_default())?;
    let (size, signed) = match acc["componentType"].as_u64() {
        Some(5120) => (1, true),
        Some(5121) => (1, false),
        Some(5122) => (2, true),
        Some(5123) => (2, false),
        other => return Err(format!("unsupported component type {other:?}")),
    };
    let normalized = acc["normalized"].as_bool().unwrap_or(false);
    let stride = match field(view, "byteStride")? {
        0 => comps * size,
        stride => stride,
    };
    let (base, count) = (field(acc, "byteOffset")?, field(acc, "count")?);
    if count == 0 {
        return Err("accessor count is zero".into());
    }
    // The last element must lie inside the view; this also bounds `count` by the data present.
    let overrun = || "accessor overruns its buffer view";
    (count - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(base))
        .and_then(|n| n.checked_add(comps * size))
        .filter(|end| *end <= region.len())
        .ok_or_else(overrun)?;
    let mut floats = Vec::with_capacity(count * comps);
    for i in 0..count {
        for c in 0..comps {
            let at = base + i * stride + c * size;
            let raw = &region[at..at + size];
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

/// Loads GLBs like `bevy_gltf`, after expanding meshopt and quantization.
/// Registered after `GltfPlugin`, so it takes over `.gltf`/`.glb`; only GLBs
/// are decoded (see `decode_compressed_glb`).
#[derive(TypePath)]
pub struct CompressedGltfLoader(GltfLoader);

impl CompressedGltfLoader {
    pub fn new(loader: GltfLoader) -> Self {
        Self(loader)
    }
}

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

    /// A one-view GLB claiming `QUANTIZATION`, so it goes through the decoder.
    fn glb_with_view(view: Value, bin: &[u8]) -> Vec<u8> {
        let doc = json!({
            "asset": { "version": "2.0" },
            "extensionsUsed": [QUANTIZATION],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [view],
        });
        write_glb(&doc, bin).unwrap()
    }

    fn meshopt_triangles(count: u64, stride: u64, view_len: u64) -> Vec<u8> {
        glb_with_view(
            json!({
                "buffer": 1,
                "byteLength": view_len,
                "extensions": { MESHOPT: {
                    "buffer": 0, "byteOffset": 0, "byteLength": 8,
                    "count": count, "byteStride": stride, "mode": "TRIANGLES",
                } },
            }),
            &[0; 8],
        )
    }

    #[test]
    fn rejects_a_truncated_chunk() {
        let err = decode_compressed_glb(&COMPRESSED[..COMPRESSED.len() - 10]).unwrap_err();
        assert!(err.contains("overruns"), "{err}");
    }

    #[test]
    fn rejects_a_view_past_the_bin_chunk() {
        let glb = glb_with_view(
            json!({ "buffer": 0, "byteOffset": 1000, "byteLength": 4 }),
            &[0; 8],
        );
        let err = decode_compressed_glb(&glb).unwrap_err();
        assert!(err.contains("overruns"), "{err}");
    }

    #[test]
    fn rejects_a_triangle_count_not_divisible_by_three() {
        let err = decode_compressed_glb(&meshopt_triangles(4, 2, 8)).unwrap_err();
        assert!(err.contains("multiple of 3"), "{err}");
    }

    #[test]
    fn rejects_a_meshopt_view_whose_size_disagrees_with_its_count() {
        let err = decode_compressed_glb(&meshopt_triangles(3, 2, 8)).unwrap_err();
        assert!(err.contains("does not match"), "{err}");
    }
}
