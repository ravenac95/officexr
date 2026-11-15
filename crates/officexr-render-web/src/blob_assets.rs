//! The `blob` asset source: `blob://<id>.<ext>` loads the page's
//! `blob:<origin>/<id>`, so the page can hand Bevy bytes it fetched itself,
//! e.g. with credentials Bevy's own fetch doesn't send.

use std::borrow::Cow;

use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::wasm::HttpWasmAssetReader;
use bevy::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_namespace = location, js_name = origin)]
    static ORIGIN: String;
}

/// Add before `AssetPlugin`, as every asset source.
pub fn plugin(app: &mut App) {
    let origin = ORIGIN.with(String::clone);
    app.register_asset_source(
        "blob",
        AssetSourceBuilder::new(move || {
            let origin = origin.clone();
            Box::new(
                HttpWasmAssetReader::new("").with_request_mapper(move |path| {
                    // The extension only picks Bevy's loader; the blob URL has none.
                    let id = path.rsplit_once('.').map_or(path, |(id, _)| id);
                    Cow::Owned(format!("blob:{origin}/{id}"))
                }),
            )
        }),
    );
}
